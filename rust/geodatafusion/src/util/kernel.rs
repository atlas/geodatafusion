//! Per-geometry kernels and the drivers that apply them to GeoArrow arrays.
//!
//! The drivers dispatch once on the array's type and read every geometry through `geo-traits`,
//! without converting the array. They append NULL for NULL rows and propagate errors, so a
//! kernel only handles a present geometry.

use std::sync::Arc;

use arrow_schema::Field;
use datafusion::common::internal_datafusion_err;
use geo_traits::GeometryTrait;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor, downcast_geoarrow_array};
use geoarrow_schema::GeoArrowType;

use crate::error::GeoDataFusionResult;

/// The work a UDF does for one geometry.
///
/// Arguments other than the geometry are fields of the kernel, read by `row`.
pub(crate) trait GeometryKernel {
    type Output;

    /// The result for a present geometry in row `row`. `Ok(None)` is SQL NULL.
    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Self::Output>>;
}

/// Applies `kernel` to every geometry of `array` and collects the results into an Arrow array
/// such as `BooleanArray`, `Int32Array` or `Float64Array`.
pub(crate) fn map_geometry<O, K>(array: &dyn GeoArrowArray, kernel: &K) -> GeoDataFusionResult<O>
where
    K: GeometryKernel,
    O: FromIterator<Option<K::Output>>,
{
    downcast_geoarrow_array!(array, map_geometry_impl, kernel)
}

fn map_geometry_impl<'a, O, K>(
    array: &'a impl GeoArrowArrayAccessor<'a>,
    kernel: &K,
) -> GeoDataFusionResult<O>
where
    K: GeometryKernel,
    O: FromIterator<Option<K::Output>>,
{
    array
        .iter()
        .enumerate()
        .map(|(row, item)| match item {
            // SQL NULL in, SQL NULL out.
            None => Ok(None),
            Some(geom) => kernel.eval(&geom?, row),
        })
        .collect()
}

/// Applies a geometry-returning `kernel` to every geometry of `array` and writes the results as
/// WKB, with the type (and so the CRS) of `return_field`.
pub(crate) fn map_geometry_to_wkb<K>(
    array: &dyn GeoArrowArray,
    kernel: &K,
    return_field: &Field,
) -> GeoDataFusionResult<Arc<dyn GeoArrowArray>>
where
    K: GeometryKernel,
    K::Output: GeometryTrait<T = f64>,
{
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(return_field)? else {
        return Err(internal_datafusion_err!(
            "map_geometry_to_wkb needs a WKB return field, got {return_field:?}"
        )
        .into());
    };
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    downcast_geoarrow_array!(array, map_geometry_to_wkb_impl, kernel, &mut builder)?;
    Ok(Arc::new(builder.finish()))
}

fn map_geometry_to_wkb_impl<'a, K>(
    array: &'a impl GeoArrowArrayAccessor<'a>,
    kernel: &K,
    builder: &mut WkbBuilder<i32>,
) -> GeoDataFusionResult<()>
where
    K: GeometryKernel,
    K::Output: GeometryTrait<T = f64>,
{
    for (row, item) in array.iter().enumerate() {
        let output = match item {
            // SQL NULL in, SQL NULL out.
            None => None,
            Some(geom) => kernel.eval(&geom?, row)?,
        };
        builder.push_geometry(output.as_ref())?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use arrow_array::BooleanArray;
    use arrow_schema::DataType;
    use geo_traits::{CoordTrait, PointTrait};
    use geoarrow_array::array::WkbArray;
    use geoarrow_array::builder::PointBuilder;
    use geoarrow_array::cast::AsGeoArrowArray;
    use geoarrow_schema::{Dimension, PointType, WkbType};

    use super::*;

    fn points() -> Arc<dyn GeoArrowArray> {
        let mut builder = PointBuilder::new(PointType::new(Dimension::XY, Default::default()));
        builder.push_point(Some(&geo::point!(x: 1.0, y: 2.0)));
        builder.push_null();
        builder.push_point(Some(&geo::point!(x: -3.0, y: 4.0)));
        Arc::new(builder.finish())
    }

    struct PositiveX;

    impl GeometryKernel for PositiveX {
        type Output = bool;

        fn eval(
            &self,
            geom: &impl GeometryTrait<T = f64>,
            _row: usize,
        ) -> GeoDataFusionResult<Option<bool>> {
            let geo_traits::GeometryType::Point(point) = geom.as_type() else {
                return Ok(None);
            };
            Ok(point.coord().map(|coord| coord.x() > 0.0))
        }
    }

    struct Swap;

    impl GeometryKernel for Swap {
        type Output = geo::Point;

        fn eval(
            &self,
            geom: &impl GeometryTrait<T = f64>,
            _row: usize,
        ) -> GeoDataFusionResult<Option<geo::Point>> {
            let geo_traits::GeometryType::Point(point) = geom.as_type() else {
                return Ok(None);
            };
            Ok(point
                .coord()
                .map(|coord| geo::point!(x: coord.y(), y: coord.x())))
        }
    }

    #[test]
    fn test_map_geometry_propagates_nulls() {
        let result: BooleanArray = map_geometry(points().as_ref(), &PositiveX).unwrap();
        assert_eq!(
            result,
            BooleanArray::from(vec![Some(true), None, Some(false)])
        );
    }

    #[test]
    fn test_map_geometry_to_wkb_uses_return_field_type() {
        let field = Field::new("swap", DataType::Binary, true)
            .with_extension_type(WkbType::new(Default::default()));
        let result = map_geometry_to_wkb(points().as_ref(), &Swap, &field).unwrap();
        let wkb: &WkbArray = result.as_wkb::<i32>();
        assert_eq!(wkb.len(), 3);
        assert!(wkb.is_null(1));
        let swapped = wkb::reader::read_wkb(wkb.inner().value(0)).unwrap();
        let geo_traits::GeometryType::Point(point) = swapped.as_type() else {
            panic!("expected a point");
        };
        let coord = point.coord().unwrap();
        assert_eq!((coord.x(), coord.y()), (2.0, 1.0));
    }
}
