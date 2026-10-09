mod accessors;
mod affine_transformations;
mod bounding_box;
mod constructors;
mod editors;
mod io;
mod measurement;
mod operators;
mod processing;
mod relationships;
mod srs;
mod types;

use pyo3::prelude::*;

#[pymodule]
pub(crate) fn native(m: &Bound<PyModule>) -> PyResult<()> {
    // accessors
    m.add_class::<accessors::PyCoordDim>()?;
    m.add_class::<accessors::PyEndPoint>()?;
    m.add_class::<accessors::PyIsClosed>()?;
    m.add_class::<accessors::PyIsEmpty>()?;
    m.add_class::<accessors::PyDump>()?;
    m.add_class::<accessors::PyM>()?;
    m.add_class::<accessors::PyGeometryType>()?;
    m.add_class::<accessors::PyNDims>()?;
    m.add_class::<accessors::PyNPoints>()?;
    m.add_class::<accessors::PyNumPoints>()?;
    m.add_class::<accessors::PyNumInteriorRings>()?;
    m.add_class::<accessors::PyStartPoint>()?;
    m.add_class::<accessors::PySTGeometryType>()?;
    m.add_class::<accessors::PyX>()?;
    m.add_class::<accessors::PyY>()?;
    m.add_class::<accessors::PyZ>()?;
    m.add_class::<accessors::PyDimension>()?;
    m.add_class::<accessors::PyZmflag>()?;
    m.add_class::<accessors::PyHasZ>()?;
    m.add_class::<accessors::PyHasM>()?;
    m.add_class::<accessors::PyIsCollection>()?;
    m.add_class::<accessors::PyNumGeometries>()?;
    m.add_class::<accessors::PyGeometryN>()?;
    m.add_class::<accessors::PyNRings>()?;
    m.add_class::<accessors::PyExteriorRing>()?;
    m.add_class::<accessors::PyInteriorRingN>()?;
    m.add_class::<accessors::PyPointN>()?;
    m.add_class::<accessors::PyPoints>()?;
    m.add_class::<accessors::PyEnvelope>()?;
    m.add_class::<accessors::PyBoundingDiagonal>()?;
    m.add_class::<accessors::PyIsPolygonCW>()?;
    m.add_class::<accessors::PyIsPolygonCCW>()?;
    m.add_class::<accessors::PyBoundary>()?;
    m.add_class::<accessors::PySummary>()?;

    // affine_transformations
    m.add_class::<affine_transformations::PyAffine>()?;
    m.add_class::<affine_transformations::PyTranslate>()?;
    m.add_class::<affine_transformations::PyScale>()?;
    m.add_class::<affine_transformations::PyRotate>()?;
    m.add_class::<affine_transformations::PyRotateX>()?;
    m.add_class::<affine_transformations::PyRotateY>()?;
    m.add_class::<affine_transformations::PyRotateZ>()?;
    m.add_class::<affine_transformations::PyTransScale>()?;

    // bounding_box
    m.add_class::<bounding_box::PyBox2D>()?;
    m.add_class::<bounding_box::PyBox3D>()?;
    m.add_class::<bounding_box::PyExtent>()?;
    m.add_class::<bounding_box::PyExtent3D>()?;
    m.add_class::<bounding_box::PyMakeBox2D>()?;
    m.add_class::<bounding_box::PyMakeBox3D>()?;
    m.add_class::<bounding_box::PyXMax>()?;
    m.add_class::<bounding_box::PyXMin>()?;
    m.add_class::<bounding_box::PyYMax>()?;
    m.add_class::<bounding_box::PyYMin>()?;
    m.add_class::<bounding_box::PyZMax>()?;
    m.add_class::<bounding_box::PyZMin>()?;
    m.add_class::<bounding_box::PyExpand>()?;

    // constructors
    m.add_class::<constructors::PyCollect>()?;
    m.add_class::<constructors::PyCollectAgg>()?;
    m.add_class::<constructors::PyMakeLine>()?;
    m.add_class::<constructors::PyMakeLineAgg>()?;
    m.add_class::<constructors::PyPoint>()?;
    m.add_class::<constructors::PyPointZ>()?;
    m.add_class::<constructors::PyPointM>()?;
    m.add_class::<constructors::PyPointZM>()?;
    m.add_class::<constructors::PyMakePoint>()?;
    m.add_class::<constructors::PyMakePointM>()?;
    m.add_class::<constructors::PyMakeEnvelope>()?;
    m.add_class::<constructors::PyMakePolygon>()?;
    m.add_class::<constructors::PyPolygon>()?;
    m.add_class::<constructors::PyTileEnvelope>()?;

    // editors
    m.add_class::<editors::PyFlipCoordinates>()?;
    m.add_class::<editors::PySwapOrdinates>()?;
    m.add_class::<editors::PyForce2D>()?;
    m.add_class::<editors::PyForce3DZ>()?;
    m.add_class::<editors::PyForce3DM>()?;
    m.add_class::<editors::PyForce4D>()?;
    m.add_class::<editors::PyShiftLongitude>()?;
    m.add_class::<editors::PyReverse>()?;
    m.add_class::<editors::PyForcePolygonCW>()?;
    m.add_class::<editors::PyForcePolygonCCW>()?;
    m.add_class::<editors::PySnapToGrid>()?;
    m.add_class::<editors::PyQuantizeCoordinates>()?;
    m.add_class::<editors::PyMulti>()?;
    m.add_class::<editors::PyForceCollection>()?;
    m.add_class::<editors::PyCollectionExtract>()?;
    m.add_class::<editors::PyCollectionHomogenize>()?;
    m.add_class::<editors::PyProject>()?;
    m.add_class::<editors::PyAddPoint>()?;
    m.add_class::<editors::PySetPoint>()?;
    m.add_class::<editors::PyRemovePoint>()?;

    // io
    m.add_class::<io::PyAsBinary>()?;
    m.add_class::<io::PyAsEWKB>()?;
    m.add_class::<io::PyAsHEXEWKB>()?;
    m.add_class::<io::PyAsEWKT>()?;
    m.add_class::<io::PyAsText>()?;
    m.add_class::<io::PyGeomFromEWKB>()?;
    m.add_class::<io::PyGeomFromEWKT>()?;
    m.add_class::<io::PyGeomFromText>()?;
    m.add_class::<io::PyPointFromText>()?;
    m.add_class::<io::PyLineFromText>()?;
    m.add_class::<io::PyPolygonFromText>()?;
    m.add_class::<io::PyMPointFromText>()?;
    m.add_class::<io::PyMLineFromText>()?;
    m.add_class::<io::PyMPolyFromText>()?;
    m.add_class::<io::PyGeomCollFromText>()?;
    m.add_class::<io::PyGeomFromWKB>()?;
    m.add_class::<io::PyPointFromWKB>()?;
    m.add_class::<io::PyLineFromWKB>()?;
    m.add_class::<io::PyPolyFromWKB>()?;
    m.add_class::<io::PyMPointFromWKB>()?;
    m.add_class::<io::PyMLineFromWKB>()?;
    m.add_class::<io::PyMPolyFromWKB>()?;
    m.add_class::<io::PyGeomCollFromWKB>()?;
    m.add_class::<io::PyGeoHash>()?;
    m.add_class::<io::PyPointFromGeoHash>()?;
    m.add_class::<io::PyGeomFromGeoHash>()?;
    m.add_class::<io::PyBox2DFromGeoHash>()?;
    m.add_class::<io::PyAsGeoJSON>()?;
    m.add_class::<io::PyGeomFromGeoJSON>()?;
    m.add_class::<io::PyAsEncodedPolyline>()?;
    m.add_class::<io::PyLineFromEncodedPolyline>()?;

    // measurement
    m.add_class::<measurement::PyArea>()?;
    m.add_class::<measurement::PyDistance>()?;
    m.add_class::<measurement::PyLength>()?;

    // operators
    m.add_class::<operators::PyGeometryAbove>()?;
    m.add_class::<operators::PyGeometryBelow>()?;
    m.add_class::<operators::PyGeometryContains>()?;
    m.add_class::<operators::PyGeometryDistanceBox>()?;
    m.add_class::<operators::PyGeometryLeft>()?;
    m.add_class::<operators::PyGeometryOverAbove>()?;
    m.add_class::<operators::PyGeometryOverBelow>()?;
    m.add_class::<operators::PyGeometryOverLeft>()?;
    m.add_class::<operators::PyGeometryOverRight>()?;
    m.add_class::<operators::PyGeometryOverlaps>()?;
    m.add_class::<operators::PyGeometryOverlapsNd>()?;
    m.add_class::<operators::PyGeometryRight>()?;
    m.add_class::<operators::PyGeometrySame>()?;
    m.add_class::<operators::PyGeometryWithin>()?;

    // processing
    m.add_class::<processing::PySimplify>()?;
    m.add_class::<processing::PySimplifyVW>()?;

    // relationships
    m.add_class::<relationships::PyDWithin>()?;
    m.add_class::<relationships::PyRelateMatch>()?;

    // srs
    m.add_class::<srs::PySetSRID>()?;
    m.add_class::<srs::PySRID>()?;

    // types
    m.add_class::<types::PyGeometry>()?;

    Ok(())
}
