//! Further geometry arguments of GEOS-backed UDFs.

use arrow_schema::Field;
use datafusion::logical_expr::ColumnarValue;
use geos::Geometry;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::to_geos;
use crate::util::owned::OwnedColumn;

/// A geometry argument as GEOS geometries, one per row, next to owned copies of the input (for
/// PostGIS's shortcuts that return an input unchanged). A constant is converted once.
pub(crate) struct GeosColumn {
    owned: OwnedColumn,
    geos: Vec<Option<Geometry>>,
}

impl GeosColumn {
    pub(crate) fn try_new(
        value: &ColumnarValue,
        field: &Field,
        number_rows: usize,
    ) -> GeoDataFusionResult<Self> {
        let owned = OwnedColumn::try_new(value, field, number_rows)?;
        let geos = owned
            .rows()
            .iter()
            .map(|geom| geom.as_ref().map(to_geos).transpose())
            .collect::<GeoDataFusionResult<_>>()?;
        Ok(Self { owned, geos })
    }

    /// The input geometry and its GEOS conversion in row `row`, or `None` for SQL NULL.
    pub(crate) fn get(&self, row: usize) -> Option<(&Wkt<f64>, &Geometry)> {
        let geos = self.geos.get(self.owned.index(row))?.as_ref()?;
        Some((self.owned.get(row)?, geos))
    }
}
