use anyhow::Result;

use super::{
    abstract_serial_read::{SerialDataReader, SerialDataReaderError},
    abstract_serial_write::SerialDataWriter,
};

/// Combines the read and write operations for a serial collection.
///
/// Search-based insertion requires the records to be ordered according to the
/// comparison used by the operation.
pub trait SerialDataContainer<T>: SerialDataReader<T> + SerialDataWriter<T> {
    /// Replaces an equal record or inserts `value` at its sorted position.
    fn serial_auto_insert(&mut self, value: T) -> Result<()>
    where
        T: PartialOrd<T>,
    {
        let (index, found) = find_insert_position(self, &value)?;
        if found {
            self.serial_write_at(index, value)
        } else {
            self.serial_insert(index, value)
        }
    }

    /// Reads, transforms, and replaces the record at `index`.
    ///
    /// The update closure is not called if the read fails. Its error is
    /// returned without attempting a write.
    fn serial_update<F>(&mut self, index: usize, update: F) -> Result<()>
    where
        F: FnOnce(T) -> Result<T>,
    {
        let current = self.serial_get(index)?;
        let replacement = update(current)?;
        self.serial_write_at(index, replacement)
    }

    /// Reads, transforms, and replaces the record matching `key`.
    ///
    /// If there is no matching record, `update` receives `None` and its result
    /// is inserted at the key's discovered sorted position. The result must
    /// compare consistently with `key`, since the discovered position is
    /// reused rather than searched again. The update closure is called exactly
    /// once.
    fn serial_update_by_key<K, F>(&mut self, key: &K, update: F) -> Result<()>
    where
        T: PartialOrd<K>,
        F: FnOnce(Option<T>) -> Result<T>,
    {
        let (index, found) = find_insert_position(self, key)?;
        let current = if found {
            Some(self.serial_get(index)?)
        } else {
            None
        };
        let replacement = update(current)?;

        if found {
            self.serial_write_at(index, replacement)
        } else {
            self.serial_insert(index, replacement)
        }
    }

    /// Replaces a record matching `key`, or inserts `value` at its sorted
    /// position, returning the affected index.
    ///
    /// `value` must be consistent with the key ordering so the resulting
    /// collection remains sorted.
    fn serial_upsert<K>(&mut self, key: &K, value: T) -> Result<usize>
    where
        T: PartialOrd<K>,
    {
        let (index, found) = find_insert_position(self, key)?;
        if found {
            self.serial_write_at(index, value)?;
        } else {
            self.serial_insert(index, value)?;
        }
        Ok(index)
    }
}

/// Provides borrowed mutable access when a backend can guarantee a stable
/// reference into its storage.
pub trait SerialDataContainerMut<T>: SerialDataContainer<T> {
    /// Returns mutable access to the record at `index`.
    fn serial_get_mut(&mut self, index: usize) -> Result<&mut T>;
}

fn find_insert_position<C, T, K>(container: &C, key: &K) -> Result<(usize, bool)>
where
    C: SerialDataReader<T> + ?Sized,
    T: PartialOrd<K>,
{
    let len = container.serial_len();
    let mut lower = 0;
    let mut upper = len;
    let mut upper_ordering = None;

    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        let record = container.serial_get(middle)?;
        let ordering = record
            .partial_cmp(key)
            .ok_or(SerialDataReaderError::UndefinedOrdering)?;
        match ordering {
            std::cmp::Ordering::Less => lower = middle + 1,
            std::cmp::Ordering::Equal | std::cmp::Ordering::Greater => {
                upper = middle;
                upper_ordering = Some(ordering);
            }
        }
    }

    let found = lower < len && upper_ordering == Some(std::cmp::Ordering::Equal);

    Ok((lower, found))
}

impl<T: Clone> SerialDataContainer<T> for Vec<T> {}

impl<T: Clone> SerialDataContainerMut<T> for Vec<T> {
    fn serial_get_mut(&mut self, index: usize) -> Result<&mut T> {
        let len = self.len();
        self.get_mut(index)
            .ok_or(SerialDataReaderError::IndexOutOfBounds { index, len }.into())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, cmp::Ordering};

    use anyhow::{Result, anyhow};

    use super::{SerialDataContainer, SerialDataContainerMut};
    use crate::adapter::{
        abstract_serial_read::{SerialDataReader, SerialDataReaderError},
        abstract_serial_write::{SerialDataWriter, SerialDataWriterError},
    };

    fn read_and_write<C: SerialDataContainer<i32>>(container: &mut C) -> Result<()> {
        let first = container.serial_first()?;
        container.serial_write(first + 1)
    }

    #[test]
    fn container_composes_reader_and_writer() {
        let mut values = vec![10, 20];

        read_and_write(&mut values).unwrap();

        assert_eq!(values, [10, 20, 11]);
    }

    #[test]
    fn auto_insert_orders_and_replaces_records() {
        let mut values = Vec::new();
        values.serial_auto_insert(20).unwrap();
        values.serial_auto_insert(10).unwrap();
        values.serial_auto_insert(30).unwrap();
        values.serial_auto_insert(20).unwrap();

        assert_eq!(values, [10, 20, 30]);
    }

    #[test]
    fn update_reads_transforms_and_writes_record() {
        let mut values = vec![10, 20, 30];

        values.serial_update(1, |record| Ok(record + 5)).unwrap();

        assert_eq!(values, [10, 25, 30]);
    }

    #[test]
    fn update_by_key_updates_or_auto_inserts() {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd)]
        struct Entry {
            key: i32,
            value: &'static str,
        }

        impl PartialEq<i32> for Entry {
            fn eq(&self, other: &i32) -> bool {
                self.key == *other
            }
        }

        impl PartialOrd<i32> for Entry {
            fn partial_cmp(&self, other: &i32) -> Option<Ordering> {
                self.key.partial_cmp(other)
            }
        }

        let mut entries = vec![
            Entry {
                key: 10,
                value: "ten",
            },
            Entry {
                key: 30,
                value: "thirty",
            },
        ];

        entries
            .serial_update_by_key(&30, |current| {
                let mut entry = current.expect("key should exist");
                entry.value = "updated";
                Ok(entry)
            })
            .unwrap();
        entries
            .serial_update_by_key(&20, |current| {
                assert!(current.is_none());
                Ok(Entry {
                    key: 20,
                    value: "twenty",
                })
            })
            .unwrap();

        assert!(entries[1] == 20);
        assert_eq!(
            entries.iter().map(|entry| entry.key).collect::<Vec<_>>(),
            [10, 20, 30]
        );
        assert_eq!(entries[2].value, "updated");
    }

    #[test]
    fn update_by_key_reuses_the_missing_key_insertion_position() {
        let mut values = simulated(vec![10, 30]);

        values
            .serial_update_by_key(&20, |record| {
                assert_eq!(record, None);
                Ok(20)
            })
            .unwrap();

        assert_eq!(values.records, [10, 20, 30]);
        assert_eq!(values.read_count.get(), 2);
    }

    #[test]
    fn simulated_backend_reader_and_writer_contract_handles_boundaries() {
        let mut backend = simulated(vec![10, 20]);

        assert_eq!(backend.serial_len(), 2);
        assert_eq!(backend.serial_get(0).unwrap(), 10);
        assert!(backend.serial_get(2).is_err());
        assert_eq!(
            backend
                .serial_iter()
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            [10, 20]
        );

        backend.serial_write(30).unwrap();
        backend.fail_write = true;
        assert!(backend.serial_write(40).is_err());
        backend.fail_write = false;
        backend.serial_insert(backend.serial_len(), 40).unwrap();
        assert!(backend.serial_insert(backend.serial_len() + 1, 50).is_err());
        backend.serial_write_at(0, 5).unwrap();
        assert!(backend.serial_write_at(backend.serial_len(), 0).is_err());
        backend.serial_remove(0).unwrap();
        assert!(backend.serial_remove(backend.serial_len()).is_err());
        assert_eq!(backend.serial_remove_get(0).unwrap(), 20);
        assert!(backend.serial_remove_get(backend.serial_len()).is_err());
        backend.serial_truncate(1).unwrap();
        backend.serial_truncate(0).unwrap();
        backend.serial_clear().unwrap();
        assert!(backend.records.is_empty());
    }

    #[test]
    fn upsert_inserts_or_replaces_by_key() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        struct Entry {
            key: i32,
            value: &'static str,
        }

        impl PartialEq<i32> for Entry {
            fn eq(&self, other: &i32) -> bool {
                self.key == *other
            }
        }

        impl PartialOrd<i32> for Entry {
            fn partial_cmp(&self, other: &i32) -> Option<Ordering> {
                self.key.partial_cmp(other)
            }
        }

        let mut entries = vec![
            Entry {
                key: 10,
                value: "ten",
            },
            Entry {
                key: 30,
                value: "thirty",
            },
        ];

        assert_eq!(
            entries
                .serial_upsert(
                    &20,
                    Entry {
                        key: 20,
                        value: "twenty",
                    },
                )
                .unwrap(),
            1
        );
        assert_eq!(
            entries
                .serial_upsert(
                    &30,
                    Entry {
                        key: 30,
                        value: "updated",
                    },
                )
                .unwrap(),
            2
        );
        assert_eq!(
            entries
                .serial_upsert(
                    &5,
                    Entry {
                        key: 5,
                        value: "five",
                    },
                )
                .unwrap(),
            0
        );
        assert_eq!(
            entries
                .serial_upsert(
                    &40,
                    Entry {
                        key: 40,
                        value: "forty",
                    },
                )
                .unwrap(),
            4
        );

        assert_eq!(
            entries.iter().map(|entry| entry.key).collect::<Vec<_>>(),
            [5, 10, 20, 30, 40]
        );
        assert!(entries[2] == 20);
        assert_eq!(entries[3].value, "updated");
    }

    #[test]
    fn mutable_extension_returns_checked_vector_access() {
        let mut values = vec![10, 20];
        *values.serial_get_mut(1).unwrap() = 25;

        assert_eq!(values, [10, 25]);
        assert_eq!(
            values
                .serial_get_mut(2)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 2, len: 2 })
        );
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd)]
    struct NonCloneRecord(i32);

    struct NonCloneBackend {
        records: Vec<i32>,
    }

    fn decode_non_clone_record(value: &i32) -> Result<NonCloneRecord> {
        Ok(NonCloneRecord(*value))
    }

    impl SerialDataReader<NonCloneRecord> for NonCloneBackend {
        type Iter<'a>
            = std::iter::Map<std::slice::Iter<'a, i32>, fn(&'a i32) -> Result<NonCloneRecord>>
        where
            Self: 'a;

        fn serial_len(&self) -> usize {
            self.records.len()
        }

        fn serial_get(&self, index: usize) -> Result<NonCloneRecord> {
            self.records.get(index).copied().map(NonCloneRecord).ok_or(
                SerialDataReaderError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into(),
            )
        }

        fn serial_iter(&self) -> Result<Self::Iter<'_>> {
            Ok(self.records.iter().map(decode_non_clone_record))
        }
    }

    impl SerialDataWriter<NonCloneRecord> for NonCloneBackend {
        fn serial_write(&mut self, value: NonCloneRecord) -> Result<()> {
            self.records.push(value.0);
            Ok(())
        }

        fn serial_insert(&mut self, index: usize, value: NonCloneRecord) -> Result<()> {
            if index > self.records.len() {
                return Err(SerialDataWriterError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into());
            }
            self.records.insert(index, value.0);
            Ok(())
        }

        fn serial_write_at(&mut self, index: usize, value: NonCloneRecord) -> Result<()> {
            let len = self.records.len();
            let record = self
                .records
                .get_mut(index)
                .ok_or(SerialDataWriterError::IndexOutOfBounds { index, len })?;
            *record = value.0;
            Ok(())
        }

        fn serial_remove(&mut self, index: usize) -> Result<()> {
            self.serial_remove_get(index).map(drop)
        }

        fn serial_remove_get(&mut self, index: usize) -> Result<NonCloneRecord> {
            if index >= self.records.len() {
                return Err(SerialDataWriterError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into());
            }
            Ok(NonCloneRecord(self.records.remove(index)))
        }

        fn serial_clear(&mut self) -> Result<()> {
            self.records.clear();
            Ok(())
        }

        fn serial_truncate(&mut self, len: usize) -> Result<()> {
            self.records.truncate(len);
            Ok(())
        }
    }

    impl SerialDataContainer<NonCloneRecord> for NonCloneBackend {}

    #[test]
    fn container_operations_support_non_clone_records() {
        let mut backend = NonCloneBackend {
            records: vec![10, 30],
        };
        backend.serial_auto_insert(NonCloneRecord(20)).unwrap();
        backend
            .serial_update(1, |mut record| {
                record.0 += 1;
                Ok(record)
            })
            .unwrap();

        assert_eq!(backend.records, [10, 21, 30]);
    }

    #[test]
    fn non_clone_backend_reader_and_writer_contract_handles_boundaries() {
        let mut backend = NonCloneBackend {
            records: vec![10, 20],
        };

        assert_eq!(backend.serial_len(), 2);
        assert_eq!(backend.serial_get(0).unwrap(), NonCloneRecord(10));
        assert_eq!(
            backend
                .serial_iter()
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            [NonCloneRecord(10), NonCloneRecord(20)]
        );
        assert!(backend.serial_get(2).is_err());

        backend.serial_write(NonCloneRecord(30)).unwrap();
        backend.serial_insert(0, NonCloneRecord(5)).unwrap();
        assert!(
            backend
                .serial_insert(backend.serial_len() + 1, NonCloneRecord(0))
                .is_err()
        );
        backend.serial_write_at(1, NonCloneRecord(10)).unwrap();
        assert!(
            backend
                .serial_write_at(backend.serial_len(), NonCloneRecord(0))
                .is_err()
        );
        backend.serial_remove(0).unwrap();
        assert!(backend.serial_remove(backend.serial_len()).is_err());
        assert_eq!(backend.serial_remove_get(1).unwrap(), NonCloneRecord(20));
        assert!(backend.serial_remove_get(backend.serial_len()).is_err());

        backend.serial_truncate(1).unwrap();
        backend.serial_truncate(10).unwrap();
        backend.serial_clear().unwrap();
        assert!(backend.records.is_empty());
    }

    struct SimulatedBackend {
        records: Vec<i32>,
        fail_read_at: Option<usize>,
        fail_write: bool,
        read_count: Cell<usize>,
    }

    fn clone_record(record: &i32) -> Result<i32> {
        Ok(*record)
    }

    impl SerialDataReader<i32> for SimulatedBackend {
        type Iter<'a>
            = std::iter::Map<std::slice::Iter<'a, i32>, fn(&'a i32) -> Result<i32>>
        where
            Self: 'a;

        fn serial_len(&self) -> usize {
            self.records.len()
        }

        fn serial_get(&self, index: usize) -> Result<i32> {
            self.read_count.set(self.read_count.get() + 1);
            if self.fail_read_at == Some(index) {
                return Err(anyhow!("simulated read failure"));
            }
            self.records.get(index).copied().ok_or_else(|| {
                SerialDataReaderError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into()
            })
        }

        fn serial_iter(&self) -> Result<Self::Iter<'_>> {
            Ok(self.records.iter().map(clone_record))
        }
    }

    impl SerialDataWriter<i32> for SimulatedBackend {
        fn serial_write(&mut self, value: i32) -> Result<()> {
            if self.fail_write {
                return Err(anyhow!("simulated write failure"));
            }
            self.records.push(value);
            Ok(())
        }

        fn serial_insert(&mut self, index: usize, value: i32) -> Result<()> {
            if self.fail_write {
                return Err(anyhow!("simulated write failure"));
            }
            if index > self.records.len() {
                return Err(SerialDataWriterError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into());
            }
            self.records.insert(index, value);
            Ok(())
        }

        fn serial_write_at(&mut self, index: usize, value: i32) -> Result<()> {
            if self.fail_write {
                return Err(anyhow!("simulated write failure"));
            }
            let len = self.records.len();
            let record = self
                .records
                .get_mut(index)
                .ok_or(SerialDataWriterError::IndexOutOfBounds { index, len })?;
            *record = value;
            Ok(())
        }

        fn serial_remove(&mut self, index: usize) -> Result<()> {
            self.serial_remove_get(index).map(drop)
        }

        fn serial_remove_get(&mut self, index: usize) -> Result<i32> {
            let len = self.records.len();
            if index >= len {
                return Err(SerialDataWriterError::IndexOutOfBounds { index, len }.into());
            }
            Ok(self.records.remove(index))
        }

        fn serial_clear(&mut self) -> Result<()> {
            self.records.clear();
            Ok(())
        }

        fn serial_truncate(&mut self, len: usize) -> Result<()> {
            self.records.truncate(len);
            Ok(())
        }
    }

    impl SerialDataContainer<i32> for SimulatedBackend {}

    fn simulated(records: Vec<i32>) -> SimulatedBackend {
        SimulatedBackend {
            records,
            fail_read_at: None,
            fail_write: false,
            read_count: Cell::new(0),
        }
    }

    #[test]
    fn auto_insert_propagates_read_and_write_errors() {
        let mut reader_failure = simulated(vec![10, 30]);
        reader_failure.fail_read_at = Some(0);
        assert!(reader_failure.serial_auto_insert(20).is_err());
        assert_eq!(reader_failure.records, [10, 30]);

        let mut write_failure = simulated(vec![10, 30]);
        write_failure.fail_write = true;
        assert!(
            write_failure
                .serial_auto_insert(20)
                .unwrap_err()
                .to_string()
                .contains("write failure")
        );
        assert_eq!(write_failure.records, [10, 30]);

        let mut unordered = vec![1.0_f64, f64::NAN, 3.0];
        assert!(unordered.serial_auto_insert(2.0).is_err());
    }

    #[test]
    fn update_stops_after_read_or_closure_failure_and_propagates_write_errors() {
        let mut reader_failure = simulated(vec![10, 20]);
        reader_failure.fail_read_at = Some(1);
        let mut called = false;
        assert!(
            reader_failure
                .serial_update(1, |record| {
                    called = true;
                    Ok(record + 1)
                })
                .is_err()
        );
        assert!(!called);
        assert_eq!(reader_failure.records, [10, 20]);

        let mut values = simulated(vec![10, 20]);
        assert!(
            values
                .serial_update(0, |_| Err(anyhow!("update failure")))
                .unwrap_err()
                .to_string()
                .contains("update failure")
        );
        assert_eq!(values.records, [10, 20]);

        values.fail_write = true;
        assert!(values.serial_update(0, Ok).is_err());
        assert_eq!(values.records, [10, 20]);

        assert!(values.serial_update(2, Ok).is_err());
    }

    #[test]
    fn upsert_propagates_read_comparison_and_write_errors() {
        let mut reader_failure = simulated(vec![10, 20, 30]);
        reader_failure.fail_read_at = Some(1);
        assert!(reader_failure.serial_upsert(&15, 15).is_err());
        assert_eq!(reader_failure.records, [10, 20, 30]);

        let mut write_failure = simulated(vec![10, 30]);
        write_failure.fail_write = true;
        assert!(write_failure.serial_upsert(&20, 20).is_err());
        assert_eq!(write_failure.records, [10, 30]);

        let mut unordered = vec![1.0_f64, f64::NAN, 3.0];
        assert!(unordered.serial_upsert(&2.0, 2.0).is_err());
    }

    #[test]
    fn update_by_key_propagates_read_closure_and_write_errors() {
        let mut reader_failure = simulated(vec![10, 20, 30]);
        reader_failure.fail_read_at = Some(1);
        let mut called = false;
        assert!(
            reader_failure
                .serial_update_by_key(&20, |record| {
                    called = true;
                    Ok(record.unwrap_or_default())
                })
                .is_err()
        );
        assert!(!called);
        assert_eq!(reader_failure.records, [10, 20, 30]);

        let mut closure_failure = simulated(vec![10, 30]);
        assert!(
            closure_failure
                .serial_update_by_key(&20, |_| Err(anyhow!("update failure")))
                .unwrap_err()
                .to_string()
                .contains("update failure")
        );
        assert_eq!(closure_failure.records, [10, 30]);

        let mut write_failure = simulated(vec![10, 20, 30]);
        write_failure.fail_write = true;
        assert!(
            write_failure
                .serial_update_by_key(&20, |record| Ok(record.unwrap_or_default() + 1))
                .is_err()
        );
        assert_eq!(write_failure.records, [10, 20, 30]);

        let mut missing_key_write_failure = simulated(vec![10, 30]);
        missing_key_write_failure.fail_write = true;
        assert!(
            missing_key_write_failure
                .serial_update_by_key(&20, |record| Ok(record.unwrap_or_default() + 20))
                .is_err()
        );
        assert_eq!(missing_key_write_failure.records, [10, 30]);
        assert_eq!(missing_key_write_failure.read_count.get(), 2);
    }
}
