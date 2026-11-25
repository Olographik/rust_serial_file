use pyo3::{PyResult, prelude::pyfunction, pyclass, pymethods};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

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

/// A row position in an example file, as returned by
/// `PyExampleFile.find_by_index` or built from Python with `PyPosition(index)`.
///
/// `None` means there is no position (for example a search in an empty file).
#[gen_stub_pyclass]
#[pyclass]
pub struct PyPosition(pub Option<usize>);

#[gen_stub_pymethods]
#[pymethods]
impl PyPosition {
    #[new]
    pub fn new(index: usize) -> Self {
        Self(Some(index))
    }

    /// Returns the row index, or 0 when there is no position.
    fn index(&self) -> usize {
        self.0.unwrap_or(0)
    }
}

#[cfg(test)]
mod test {
    use super::PyPosition;

    // T-007
    #[test]
    fn position_index_defaults_to_zero() {
        assert_eq!(PyPosition::new(7).index(), 7);
        assert_eq!(PyPosition(None).index(), 0);
    }
}
