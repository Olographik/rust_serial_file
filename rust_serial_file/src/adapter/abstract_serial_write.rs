use std::{error::Error, fmt};

use anyhow::Result;

/// Errors produced by the built-in `SerialDataWriter` operations.
#[derive(Debug, PartialEq, Eq)]
pub enum SerialDataWriterError {
    IndexOutOfBounds { index: usize, len: usize },
}

impl fmt::Display for SerialDataWriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexOutOfBounds { index, len } => {
                write!(
                    formatter,
                    "index {index} is out of bounds for collection of length {len}"
                )
            }
        }
    }
}

impl Error for SerialDataWriterError {}

/// Writes owned records to a logically ordered serial collection.
///
/// Operations return [`anyhow::Result`] so backend, validation, and
/// persistence failures remain observable. Backends should preserve the
/// logical collection when an operation fails where possible and document
/// their behavior when a failure can occur after storage has changed.
pub trait SerialDataWriter<T> {
    /// Appends `value` to the end of the logical collection.
    fn serial_write(&mut self, value: T) -> Result<()>;

    /// Inserts `value` before the record at `index`.
    ///
    /// Inserting at the collection length appends. An index greater than the
    /// collection length is invalid.
    fn serial_insert(&mut self, index: usize, value: T) -> Result<()>;

    /// Replaces the record at `index` with `value`.
    ///
    /// The index must identify an existing record.
    fn serial_write_at(&mut self, index: usize, value: T) -> Result<()>;

    /// Removes the record at `index` without returning it.
    ///
    /// The index must identify an existing record.
    fn serial_remove(&mut self, index: usize) -> Result<()>;

    /// Removes and returns the record at `index`.
    ///
    /// The index must identify an existing record.
    fn serial_remove_get(&mut self, index: usize) -> Result<T>;

    /// Removes all records from the collection.
    fn serial_clear(&mut self) -> Result<()>;

    /// Retains at most the first `len` records.
    ///
    /// If `len` is at least the collection length, this succeeds without
    /// changing the collection.
    fn serial_truncate(&mut self, len: usize) -> Result<()>;
}

impl<T> SerialDataWriter<T> for Vec<T> {
    fn serial_write(&mut self, value: T) -> Result<()> {
        self.push(value);
        Ok(())
    }

    fn serial_insert(&mut self, index: usize, value: T) -> Result<()> {
        if index > self.len() {
            return Err(SerialDataWriterError::IndexOutOfBounds {
                index,
                len: self.len(),
            }
            .into());
        }
        self.insert(index, value);
        Ok(())
    }

    fn serial_write_at(&mut self, index: usize, value: T) -> Result<()> {
        let len = self.len();
        let Some(record) = self.get_mut(index) else {
            return Err(SerialDataWriterError::IndexOutOfBounds { index, len }.into());
        };
        *record = value;
        Ok(())
    }

    fn serial_remove(&mut self, index: usize) -> Result<()> {
        let len = self.len();
        if index >= len {
            return Err(SerialDataWriterError::IndexOutOfBounds { index, len }.into());
        }
        self.remove(index);
        Ok(())
    }

    fn serial_remove_get(&mut self, index: usize) -> Result<T> {
        let len = self.len();
        if index >= len {
            return Err(SerialDataWriterError::IndexOutOfBounds { index, len }.into());
        }
        Ok(self.remove(index))
    }

    fn serial_clear(&mut self) -> Result<()> {
        self.clear();
        Ok(())
    }

    fn serial_truncate(&mut self, len: usize) -> Result<()> {
        self.truncate(len);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};

    use super::{SerialDataWriter, SerialDataWriterError};

    #[derive(Debug, PartialEq, Eq)]
    struct NonClone(i32);

    fn append_non_clone<C: SerialDataWriter<NonClone>>(container: &mut C) -> Result<()> {
        container.serial_write(NonClone(7))
    }

    #[test]
    fn writer_works_with_non_clone_records() {
        let mut values = Vec::new();

        append_non_clone(&mut values).unwrap();

        assert_eq!(values, [NonClone(7)]);
    }

    #[test]
    fn serial_write_appends_to_empty_and_non_empty_vectors() {
        let mut values = Vec::new();
        values.serial_write(10).unwrap();
        values.serial_write(20).unwrap();

        assert_eq!(values, [10, 20]);
    }

    #[test]
    fn serial_insert_handles_boundaries_and_rejects_out_of_range_indices() {
        let mut values = vec![10, 30];
        values.serial_insert(0, 5).unwrap();
        values.serial_insert(2, 20).unwrap();
        values.serial_insert(4, 40).unwrap();

        assert_eq!(values, [5, 10, 20, 30, 40]);

        let error = values.serial_insert(6, 60).unwrap_err();
        assert_eq!(
            error.downcast_ref::<SerialDataWriterError>(),
            Some(&SerialDataWriterError::IndexOutOfBounds { index: 6, len: 5 })
        );
        assert_eq!(values, [5, 10, 20, 30, 40]);
    }

    #[test]
    fn index_out_of_bounds_error_has_a_readable_message() {
        let error = SerialDataWriterError::IndexOutOfBounds { index: 4, len: 2 };

        assert_eq!(
            error.to_string(),
            "index 4 is out of bounds for collection of length 2"
        );
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn serial_write_at_replaces_existing_record_and_rejects_invalid_index() {
        let mut values = vec![10, 20, 30];
        values.serial_write_at(1, 25).unwrap();

        assert_eq!(values, [10, 25, 30]);

        let error = values.serial_write_at(3, 40).unwrap_err();
        assert_eq!(
            error.downcast_ref::<SerialDataWriterError>(),
            Some(&SerialDataWriterError::IndexOutOfBounds { index: 3, len: 3 })
        );
        assert_eq!(values, [10, 25, 30]);
    }

    #[test]
    fn serial_remove_removes_without_returning_value() {
        let mut values = vec![10, 20, 30, 40];
        values.serial_remove(0).unwrap();
        assert_eq!(values, [20, 30, 40]);

        values.serial_remove(1).unwrap();
        assert_eq!(values, [20, 40]);

        values.serial_remove(1).unwrap();
        assert_eq!(values, [20]);

        let error = values.serial_remove(1).unwrap_err();
        assert_eq!(
            error.downcast_ref::<SerialDataWriterError>(),
            Some(&SerialDataWriterError::IndexOutOfBounds { index: 1, len: 1 })
        );
        assert_eq!(values, [20]);
    }

    #[test]
    fn serial_remove_get_returns_removed_value() {
        let mut values = vec![10, 20, 30, 40];

        assert_eq!(values.serial_remove_get(0).unwrap(), 10);
        assert_eq!(values, [20, 30, 40]);

        assert_eq!(values.serial_remove_get(1).unwrap(), 30);
        assert_eq!(values, [20, 40]);

        assert_eq!(values.serial_remove_get(1).unwrap(), 40);
        assert_eq!(values, [20]);

        let error = values.serial_remove_get(1).unwrap_err();
        assert_eq!(
            error.downcast_ref::<SerialDataWriterError>(),
            Some(&SerialDataWriterError::IndexOutOfBounds { index: 1, len: 1 })
        );
        assert_eq!(values, [20]);
    }

    #[test]
    fn serial_clear_and_truncate_follow_collection_semantics() {
        let mut values = vec![10, 20, 30];
        values.serial_truncate(2).unwrap();
        assert_eq!(values, [10, 20]);

        values.serial_truncate(3).unwrap();
        assert_eq!(values, [10, 20]);

        values.serial_truncate(0).unwrap();
        assert!(values.is_empty());

        values.serial_clear().unwrap();
        values.extend([10, 20]);
        values.serial_clear().unwrap();
        assert!(values.is_empty());
        values.serial_clear().unwrap();
        assert!(values.is_empty());
    }

    struct SimulatedFailingWriter;

    impl SerialDataWriter<i32> for SimulatedFailingWriter {
        fn serial_write(&mut self, _value: i32) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_insert(&mut self, _index: usize, _value: i32) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_write_at(&mut self, _index: usize, _value: i32) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_remove(&mut self, _index: usize) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_remove_get(&mut self, _index: usize) -> Result<i32> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_clear(&mut self) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }

        fn serial_truncate(&mut self, _len: usize) -> Result<()> {
            Err(anyhow!("simulated backend failure"))
        }
    }

    #[test]
    fn simulated_backend_failures_are_returned_for_each_operation() {
        let mut writer = SimulatedFailingWriter;

        assert_eq!(
            writer.serial_write(10).unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_insert(0, 10).unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_write_at(0, 10).unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_remove(0).unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_remove_get(0).unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_clear().unwrap_err().to_string(),
            "simulated backend failure"
        );
        assert_eq!(
            writer.serial_truncate(0).unwrap_err().to_string(),
            "simulated backend failure"
        );
    }
}
