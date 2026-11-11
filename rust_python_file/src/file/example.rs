use std::{fmt::Debug, fs::OpenOptions};

use pyo3::{PyErr, PyResult, exceptions::PyTypeError, pyclass, pymethods};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};
use rust_serial_file::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    file::{file_metadata::FileMetadata, struct_file::StructFile},
    serialized_struct::{self, SerializedStruct},
};
use serialized_struct_derive::SerializedStruct;
use utils::search::search_type::SearchContext;

use crate::file::PyPosition;

/// Example record stored in a [`PyExampleFile`]: one little-endian `u64`.
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

/// A file of [`ExamplStruct`] records exposed to Python.
///
/// The file has no metadata prefix: rows of `ExamplStruct::SIZE` bytes start
/// at offset 0. A file opened read-only rejects writes with `TypeError`.
#[gen_stub_pyclass]
#[pyclass]
pub struct PyExampleFile {
    f: StructFile<ExamplStruct>,
    read_only: bool,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyExampleFile {
    /// Opens `file_name`. Unless `read_only` is set, the file is opened for
    /// writing too and created if missing; it is never truncated.
    #[new]
    pub fn open(file_name: &str, read_only: bool) -> PyResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .create(!read_only)
            .truncate(false)
            .open(file_name)
            .map_err(|error| open_error(file_name, &error))?;
        let metadata = FileMetadata::new(file, ExamplStruct::SIZE as u64, 0)
            .map_err(|error| open_error(file_name, &error))?;
        let f = StructFile::new(metadata).map_err(|error| open_error(file_name, &error))?;
        Ok(Self { f, read_only })
    }

    /// Returns the record at `index`, or `None` if it is out of range or
    /// cannot be read.
    pub fn read_at(&self, index: usize) -> Option<ExamplStruct> {
        self.f.read_value_at(index).ok()
    }

    /// Binary-searches the records by their `index` field.
    ///
    /// Returns the matching row, else the row just before where `index` would
    /// be, else the row just after; an empty file gives no position.
    pub fn find_by_index(&self, index: u64) -> PyResult<PyPosition> {
        let mut context = SearchContext::none();
        self.f
            .serial_find_by_with_context(&index, &mut context)
            .map_err(|error| {
                PyTypeError::new_err(format!("Could not search the file: {error:?}"))
            })?;
        Ok(PyPosition(context.0))
    }

    /// Appends a copy of `value`.
    pub fn write(&mut self, value: &ExamplStruct) -> PyResult<()> {
        self.check_writable()?;
        self.f
            .serial_write(ExamplStruct::new(value.index))
            .map_err(|error| write_error(&error))
    }

    /// Replaces the record at `position`, or appends when `position` equals
    /// the record count.
    pub fn write_at(&mut self, position: &PyPosition, value: &ExamplStruct) -> PyResult<()> {
        self.check_writable()?;
        let index = position.index();
        let value = ExamplStruct::new(value.index);
        let result = if index == self.f.serial_len() {
            self.f.serial_write(value)
        } else {
            self.f.serial_write_at(index, value)
        };
        result.map_err(|error| write_error(&error))
    }

    /// Returns the number of records.
    pub fn item_count(&self) -> usize {
        self.f.serial_len()
    }
}

impl PyExampleFile {
    fn check_writable(&self) -> PyResult<()> {
        if self.read_only {
            return Err(PyTypeError::new_err(
                "Could not write in the file: the file is opened read-only",
            ));
        }
        Ok(())
    }
}

fn open_error(file_name: &str, error: &impl Debug) -> PyErr {
    PyTypeError::new_err(format!("Could not open the file {file_name}: {error:?}"))
}

fn write_error(error: &impl Debug) -> PyErr {
    PyTypeError::new_err(format!("Could not write in the file: {error:?}"))
}

// The tests call the Rust methods directly; no Python interpreter is started,
// so error messages are checked by the Python tests instead.
#[cfg(test)]
mod test {
    use std::fs;

    use rust_serial_file::{
        adapter::abstract_serial_write::SerialDataWriter, file::struct_file::StructFile,
    };

    use super::{ExamplStruct, PyExampleFile};
    use crate::file::PyPosition;

    fn records(file: &PyExampleFile) -> Vec<u64> {
        (0..file.item_count())
            .map(|index| file.read_at(index).unwrap().index)
            .collect()
    }

    fn file_with(path: &std::path::Path, values: &[u64]) -> PyExampleFile {
        let mut file = PyExampleFile::open(path.to_str().unwrap(), false).unwrap();
        for value in values {
            file.write(&ExamplStruct::new(*value)).unwrap();
        }
        file
    }

    // T-001
    #[test]
    fn test_example_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_str = ExamplStruct { index: 10 };
        let expected_2 = ExamplStruct { index: 20 };

        let mut file = StructFile::<ExamplStruct>::open_or_create(&file_path, 0)?;
        file.serial_write(ExamplStruct::new(expected_str.index))?;
        file.serial_write(ExamplStruct::new(expected_2.index))?;

        assert_eq!(file.read_value_at(0)?, expected_str);
        assert_eq!(file.read_value_at(1)?, expected_2);

        // The old test seeked to row 2 and wrote two more records: an append.
        file.serial_write(ExamplStruct::new(expected_str.index))?;
        file.serial_write(ExamplStruct::new(expected_2.index))?;

        assert_eq!(file.read_value_at(0)?, expected_str);
        assert_eq!(file.read_value_at(1)?, expected_2);
        assert_eq!(file.read_value_at(2)?, expected_str);
        assert_eq!(file.read_value_at(3)?, expected_2);
        drop(file);
        assert_eq!(fs::metadata(&file_path)?.len(), 32);
        Ok(())
    }

    // T-002
    #[test]
    fn open_creates_or_rejects_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.bin");
        let name = path.to_str().unwrap();

        assert_eq!(PyExampleFile::open(name, false).unwrap().item_count(), 0);
        assert_eq!(PyExampleFile::open(name, true).unwrap().item_count(), 0);

        let missing = dir.path().join("missing.bin");
        assert!(PyExampleFile::open(missing.to_str().unwrap(), true).is_err());
        assert!(!missing.exists());
    }

    // T-003
    #[test]
    fn write_and_read_at_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_with(&dir.path().join("values.bin"), &[3, 1, 2]);

        assert_eq!(records(&file), [3, 1, 2]);
        assert!(file.read_at(3).is_none());
        assert_eq!(file.item_count(), 3);
        assert!(file.read_at(1).unwrap() == 1u64);
    }

    // T-004
    #[test]
    fn find_by_index_returns_nearest_position() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_with(&dir.path().join("sorted.bin"), &[10, 20, 30]);

        let found = |index| file.find_by_index(index).unwrap().index();
        assert_eq!(found(20), 1);
        assert_eq!(found(25), 1);
        assert_eq!(found(5), 0);
        assert_eq!(found(40), 2);

        let empty = file_with(&dir.path().join("empty.bin"), &[]);
        let position = empty.find_by_index(10).unwrap();
        assert_eq!(position.0, None);
        assert_eq!(position.index(), 0);

        // A file truncated behind the open file fails the search with an
        // error instead of a panic.
        let path = dir.path().join("truncated.bin");
        let truncated = file_with(&path, &[10, 20, 30]);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(0)
            .unwrap();
        assert!(truncated.find_by_index(20).is_err());
    }

    // T-005
    #[test]
    fn write_at_replaces_appends_or_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = file_with(&dir.path().join("values.bin"), &[1, 2]);

        file.write_at(&PyPosition::new(0), &ExamplStruct::new(9))
            .unwrap();
        file.write_at(&PyPosition::new(2), &ExamplStruct::new(3))
            .unwrap();
        assert_eq!(records(&file), [9, 2, 3]);

        assert!(
            file.write_at(&PyPosition::new(5), &ExamplStruct::new(0))
                .is_err()
        );
        assert_eq!(file.item_count(), 3);
    }

    // T-006
    #[test]
    fn read_only_file_rejects_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("values.bin");
        drop(file_with(&path, &[4]));
        let before = fs::read(&path).unwrap();

        let mut file = PyExampleFile::open(path.to_str().unwrap(), true).unwrap();
        assert!(file.write(&ExamplStruct::new(5)).is_err());
        assert!(
            file.write_at(&PyPosition::new(0), &ExamplStruct::new(5))
                .is_err()
        );

        assert_eq!(file.item_count(), 1);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
