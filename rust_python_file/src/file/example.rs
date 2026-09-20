use std::{fs::File, sync::Arc};

use pyo3::{PyResult, pyclass, pymethods};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};
use rust_serial_file::file::position::binary_look_up_file_struct;
use rust_serial_file::file::{FileReference, struct_file::StructFile};
use rust_serial_file::serialized_struct;
use rust_serial_file::utils::binary_search::SearchQuery;
use serialized_struct_derive::SerializedStruct;

use crate::file::PyPosition;

#[gen_stub_pyclass]
#[derive(Debug, PartialEq, SerializedStruct)]
#[pyclass]
pub struct ExamplStruct {
    #[pyo3(get, set)]
    pub index: u64,
}

impl PartialEq<u64> for ExamplStruct {
    fn eq(&self, other: &u64) -> bool {
        &self.index == other
    }
}
impl PartialOrd<u64> for ExamplStruct {
    fn partial_cmp(&self, other: &u64) -> Option<std::cmp::Ordering> {
        self.index.partial_cmp(other)
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl ExamplStruct {
    #[new]
    pub fn new(index: u64) -> Self {
        Self { index }
    }
}

#[gen_stub_pyclass]
#[pyclass]
pub struct PyExampleFile {
    f: StructFile<ExamplStruct, Arc<File>>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyExampleFile {
    #[new]
    pub fn open(file_name: &str, read_only: bool) -> PyResult<Self> {
        match FileReference::free_from_path(file_name, 0, 0, read_only) {
            Ok(f) => Ok(Self {
                f: StructFile::wrap(f),
            }),

            Err(e) => Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "Could not open the file {}: {:?}",
                file_name, e
            ))),
        }
    }

    pub fn read_at(&mut self, index: usize) -> Option<ExamplStruct> {
        let mut reader = self.f.reader(1);
        match reader.read_value_at(index) {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }

    pub fn find_by_index(&mut self, index: u64) -> PyPosition {
        let mut reader = self.f.reader(24);
        let pos = binary_look_up_file_struct(&mut reader, &index, SearchQuery::BeforeOrExact);
        PyPosition(pos.into())
    }

    pub fn write(&mut self, value: &ExamplStruct) -> PyResult<()> {
        match self.f.writer(1).write_value(value) {
            Ok(_) => Ok(()),

            Err(e) => Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "Could not write in the file: {:?}",
                e
            ))),
        }
    }

    pub fn write_at(&mut self, position: &PyPosition, value: &ExamplStruct) -> PyResult<()> {
        match self.f.writer(1).write_value_at(position.index(), value) {
            Ok(_) => Ok(()),

            Err(e) => Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "Could not write in the file: {:?}",
                e
            ))),
        }
    }

    pub fn item_count(&mut self) -> usize {
        self.f.row_count()
    }
}

#[cfg(test)]
mod test {
    use rust_serial_file::file::{FileReference, struct_file::StructFile};

    use crate::file::example::ExamplStruct;

    #[test]
    fn test_example_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_str = ExamplStruct { index: 10 };
        let expected_2 = ExamplStruct { index: 20 };

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file = file_ref.writer(4);
            //create a temp file
            write_file.write_value(&expected_str)?;
            write_file.write_value(&expected_2)?;
        }

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;

            println!("{expected_str:?} -> {result:?}");
            assert_eq!(expected_str, result);

            let result = reader.read_value_at(1)?;

            println!("{expected_2:?} -> {result:?}");
            assert_eq!(expected_2, result);
        }

        {
            let mut write_file = file_ref.writer(4);
            let _ = write_file.seek(2)?;
            //create a temp file
            write_file.write_value(&expected_str)?;
            write_file.write_value(&expected_2)?;
        }

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;

            println!("{expected_str:?} -> {result:?}");
            assert_eq!(expected_str, result);

            let result = reader.read_value_at(1)?;

            println!("{expected_2:?} -> {result:?}");
            assert_eq!(expected_2, result);

            let result = reader.read_value_at(2)?;

            println!("{expected_str:?} -> {result:?}");
            assert_eq!(expected_str, result);

            let result = reader.read_value_at(3)?;

            println!("{expected_2:?} -> {result:?}");
            assert_eq!(expected_2, result);
        }

        Ok(())
    }
}
