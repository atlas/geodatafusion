mod measurement;
mod validation;

use pyo3::prelude::*;

#[pymodule]
pub(crate) fn geo(m: &Bound<PyModule>) -> PyResult<()> {
    // measurement
    m.add_class::<measurement::PyLength>()?;

    // validation
    m.add_class::<validation::PyIsValid>()?;

    Ok(())
}
