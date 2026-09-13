use pyo3::{PyResult, prelude::pyfunction, pyclass, pymethods};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use rust_serial_file::file::position::Position;

pub mod example;

#[gen_stub_pyfunction]
#[pyfunction]
pub fn make_buffer(len: usize) -> PyResult<(usize, usize)> {
    let mut data = vec![0.0f64; len];

    for i in 0..len {
        data[i] = i as f64;
    }

    let ptr = data.as_mut_ptr() as usize;

    // prevent Rust from freeing memory
    std::mem::forget(data);

    Ok((ptr, len))
}

#[gen_stub_pyclass]
#[pyclass]
pub struct PyPosition(pub Position);

#[gen_stub_pymethods]
#[pymethods]
impl PyPosition {
    #[new]
    pub fn new(index: usize) -> Self {
        Self(Position::Exact(index))
    }

    fn index(&self) -> usize {
        self.0.into()
    }
}
