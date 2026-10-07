mod measurement;
mod processing;
mod validation;

use pyo3::prelude::*;

#[pymodule]
pub(crate) fn geo(m: &Bound<PyModule>) -> PyResult<()> {
    // measurement
    m.add_class::<measurement::PyArea>()?;
    m.add_class::<measurement::PyDistance>()?;
    m.add_class::<measurement::PyLength>()?;

    // processing
    m.add_class::<processing::PyCentroid>()?;
    m.add_class::<processing::PyConvexHull>()?;
    m.add_class::<processing::PyOrientedEnvelope>()?;
    m.add_class::<processing::PyPointOnSurface>()?;
    m.add_class::<processing::PySimplify>()?;
    m.add_class::<processing::PySimplifyPreserveTopology>()?;
    m.add_class::<processing::PySimplifyVW>()?;

    // validation
    m.add_class::<validation::PyIsValid>()?;
    m.add_class::<validation::PyIsValidReason>()?;

    Ok(())
}
