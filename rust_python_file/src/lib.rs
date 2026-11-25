pub mod file;

use pyo3::prelude::{pymodule, PyModule, PyResult, Python};
use pyo3::types::PyModuleMethods;
use pyo3::{wrap_pyfunction, Bound};
use pyo3_stub_gen::define_stub_info_gatherer;

#[pymodule]
pub fn rust_python_file(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<file::example::ExamplStruct>()?;
    m.add_class::<file::example::PyExampleFile>()?;
    m.add_class::<file::PyPosition>()?;
    m.add_function(wrap_pyfunction!(file::make_buffer, m)?)?;
    Ok(())
}

define_stub_info_gatherer!(stub_info);
