mod validation;

use pyo3::prelude::*;

#[pymodule]
pub(crate) fn geo(m: &Bound<PyModule>) -> PyResult<()> {
    // validation
    m.add_class::<validation::PyIsValid>()?;

    Ok(())
}
