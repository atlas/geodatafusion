//! The binary overlay functions: ST_Intersection, ST_Difference, ST_SymDifference and ST_Union.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geos::{Geom, Geometry};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_float_arg;
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Intersection(geometry geom1, geometry geom2, float8 gridSize = -1), and the same
/// for ST_Difference, ST_SymDifference and ST_Union.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2", "gridSize"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a geometry representing the point-set intersection of two geometries: the portion of geom1 and geom2 they share.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a geometry representing the point-set intersection of two geometries: the portion of geom1 and geom2 they share. If the optional gridSize argument is given (and not negative), the inputs are snapped to a grid of that size and the result is computed on it. This function keeps Z and drops M.",
    syntax_example = "ST_Intersection(geom1, geom2, gridSize)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "gridSize", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Intersection;

impl Intersection {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Intersection {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Intersection {
    fn name(&self) -> &str {
        "st_intersection"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(overlay_impl(self.name(), args, Operation::Intersection)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a geometry representing the part of geom1 that does not intersect geom2.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a geometry representing the part of geom1 that does not intersect geom2. If the optional gridSize argument is given (and not negative), the inputs are snapped to a grid of that size and the result is computed on it. This function keeps Z and drops M.",
    syntax_example = "ST_Difference(geom1, geom2, gridSize)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "gridSize", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Difference;

impl Difference {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Difference {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Difference {
    fn name(&self) -> &str {
        "st_difference"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(overlay_impl(self.name(), args, Operation::Difference)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a geometry representing the portions of geom1 and geom2 that do not intersect.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a geometry representing the portions of geom1 and geom2 that do not intersect. If the optional gridSize argument is given (and not negative), the inputs are snapped to a grid of that size and the result is computed on it. This function keeps Z and drops M.",
    syntax_example = "ST_SymDifference(geom1, geom2, gridSize)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "gridSize", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SymDifference;

impl SymDifference {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SymDifference {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SymDifference {
    fn name(&self) -> &str {
        "st_symdifference"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(overlay_impl(self.name(), args, Operation::SymDifference)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the point-set union of two geometries.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a geometry representing the point-set union of two geometries. If one is empty, the other is returned unchanged. If the optional gridSize argument is given (and not negative), the inputs are snapped to a grid of that size and the result is computed on it. This function keeps Z and drops M. For the aggregate form, see ST_Union_Agg; the geometry[] form isn't supported.",
    syntax_example = "ST_Union(geom1, geom2, gridSize)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "gridSize", description = "float8"),
    related_udf(name = "st_union_agg")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Union;

impl Union {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Union {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Union {
    fn name(&self) -> &str {
        "st_union"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(overlay_impl(self.name(), args, Operation::Union)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Intersection,
    Difference,
    SymDifference,
    Union,
}

fn overlay_impl(
    name: &str,
    args: ScalarFunctionArgs,
    operation: Operation,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = OverlayKernel {
        operation,
        other: GeosColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        // A negative grid size means none, as in PostGIS.
        grid_size: optional_float_arg(&args, 2, -1.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct OverlayKernel {
    operation: Operation,
    other: GeosColumn,
    grid_size: Float64Array,
}

impl GeometryKernel for OverlayKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // These functions are STRICT: SQL NULL in any argument gives SQL NULL.
        let Some((other, other_geos)) = self.other.get(row) else {
            return Ok(None);
        };
        // PostGIS keeps a Z from GEOS only when an input has Z.
        let want_z = has_z(geom) || has_z(other);
        if self.grid_size.is_null(row) {
            return Ok(None);
        }
        // PostGIS's shortcuts for EMPTY inputs return an input unchanged, M included.
        let (empty, other_empty) = (
            is_geometry_topologically_empty(geom),
            is_geometry_topologically_empty(other),
        );
        let shortcut = match self.operation {
            Operation::Intersection if other_empty => Some(other.clone()),
            Operation::Intersection if empty => Some(to_owned_geometry(geom)),
            Operation::Difference if empty || other_empty => Some(to_owned_geometry(geom)),
            Operation::SymDifference if other_empty => Some(to_owned_geometry(geom)),
            Operation::SymDifference if empty => Some(other.clone()),
            Operation::Union if empty => Some(other.clone()),
            Operation::Union if other_empty => Some(to_owned_geometry(geom)),
            _ => None,
        };
        if shortcut.is_some() {
            return Ok(shortcut);
        }
        let geom = to_geos(geom)?;
        let grid_size = self.grid_size.value(row);
        let result = overlay(&geom, other_geos, self.operation, grid_size)?;
        Ok(Some(from_geos(&result, want_z)?))
    }
}

fn overlay(
    geom: &Geometry,
    other: &Geometry,
    operation: Operation,
    grid_size: f64,
) -> GeoDataFusionResult<Geometry> {
    let snapped = grid_size >= 0.0;
    Ok(match operation {
        Operation::Intersection if snapped => geom.intersection_prec(other, grid_size)?,
        Operation::Intersection => geom.intersection(other)?,
        Operation::Difference if snapped => geom.difference_prec(other, grid_size)?,
        Operation::Difference => geom.difference(other)?,
        Operation::SymDifference if snapped => geom.sym_difference_prec(other, grid_size)?,
        Operation::SymDifference => geom.sym_difference(other)?,
        Operation::Union if snapped => geom.union_prec(other, grid_size)?,
        Operation::Union => geom.union(other)?,
    })
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_overlay_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(Intersection.into());
        ctx.register_udf(GeomFromText::new().into());

        let sql = "SELECT ST_Intersection(ST_GeomFromText('POINT(1 1)', 4326), \
                   ST_GeomFromText('MULTIPOINT((1 1),(2 2))', 4326))";
        assert_wkb_output(&ctx, sql, 4326).await;
    }
}
