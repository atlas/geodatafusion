mod accessors;
mod bounding_box;
mod constructors;
mod io;
mod measurement;
mod relationships;
mod srs;

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

    // constructors
    m.add_class::<constructors::PyPoint>()?;
    m.add_class::<constructors::PyPointZ>()?;
    m.add_class::<constructors::PyPointM>()?;
    m.add_class::<constructors::PyPointZM>()?;
    m.add_class::<constructors::PyMakePoint>()?;
    m.add_class::<constructors::PyMakePointM>()?;

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

    // measurement
    m.add_class::<measurement::PyArea>()?;
    m.add_class::<measurement::PyDistance>()?;

    // relationships
    m.add_class::<relationships::PyDWithin>()?;
    m.add_class::<relationships::PyRelateMatch>()?;

    // srs
    m.add_class::<srs::PySetSRID>()?;
    m.add_class::<srs::PySRID>()?;

    Ok(())
}
