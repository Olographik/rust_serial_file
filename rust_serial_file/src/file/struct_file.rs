use std::{marker::PhantomData, path::Path};

use anyhow::{Context, Result, bail};

use crate::{
    adapter::{
        abstract_data_container::SerialDataContainer, abstract_serial_read::SerialDataReader,
        abstract_serial_write::SerialDataWriter,
    },
    serialized_struct::SerializedStruct,
};

use super::buffered_reader::BufferedReader;
use super::file_metadata::FileMetadata;

const SHIFT_BUFFER_BYTES: usize = 64 * 1024;
const STACK_ROW_BUFFER_BYTES: usize = 1024;

/// A typed serial collection that owns its fixed-width file metadata.
pub struct StructFile<T: SerializedStruct> {
    metadata: FileMetadata,
    len: usize,
    _record: PhantomData<T>,
}

impl<T: SerializedStruct> StructFile<T> {
    /// Wraps file metadata after checking its row width against `T::SIZE`.
    pub fn new(metadata: FileMetadata) -> Result<Self> {
        if T::SIZE == 0 {
            bail!("serialized row size must be greater than zero");
        }
        if u64::try_from(T::SIZE)? != metadata.element_size() {
            bail!(
                "file row width ({}) does not match serialized row size ({})",
                metadata.element_size(),
                T::SIZE
            );
        }

        let len = usize::try_from(metadata.element_count()?)
            .context("file row count does not fit in usize")?;
        Ok(Self {
            metadata,
            len,
            _record: PhantomData,
        })
    }

    /// Opens or creates a file with rows encoded as `T`.
    pub fn open_or_create(path: impl AsRef<Path>, data_offset: u64) -> Result<Self> {
        let element_size =
            u64::try_from(T::SIZE).context("serialized row size does not fit in u64")?;
        let metadata = FileMetadata::open_or_create(path, element_size, data_offset)?;
        Self::new(metadata)
    }

    /// Returns immutable access to the owned file metadata.
    pub fn file_metadata(&self) -> &FileMetadata {
        &self.metadata
    }

    /// Consumes the collection and returns its file metadata.
    pub fn into_file_metadata(self) -> FileMetadata {
        self.metadata
    }

    /// Reads and decodes the record at `index`.
    pub fn read_value_at(&mut self, index: usize) -> Result<T> {
        self.validate_existing_index(index)?;
        let file_index = to_file_index(index)?;
        if T::SIZE <= STACK_ROW_BUFFER_BYTES {
            let mut row = [0; STACK_ROW_BUFFER_BYTES];
            self.metadata.read_row(file_index, &mut row[..T::SIZE])?;
            T::try_decode(&row[..T::SIZE])
        } else {
            let mut row = allocate_bytes(T::SIZE, "record row")?;
            self.metadata.read_row(file_index, &mut row)?;
            T::try_decode(&row)
        }
    }

    /// Encodes and replaces the record at `index`.
    pub fn write_value_at(&mut self, index: usize, value: &T) -> Result<()> {
        self.validate_existing_index(index)?;
        let file_index = to_file_index(index)?;
        with_encoded_value(value, |row| self.metadata.write_row(file_index, row))
    }

    /// Encodes records in slice order, replacing from row zero and appending
    /// any suffix beyond the current collection.
    pub fn write_multiple(&mut self, values: &[T]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }

        let byte_len = values
            .len()
            .checked_mul(T::SIZE)
            .context("encoded block byte length overflow")?;
        let row_count =
            u64::try_from(values.len()).context("record count does not fit in file row index")?;
        let new_len =
            usize::try_from(row_count).context("record count does not fit in collection length")?;
        let new_len = self.len.max(new_len);
        let mut rows = allocate_bytes(byte_len, "encoded record block")?;

        for (index, value) in values.iter().enumerate() {
            let start = index
                .checked_mul(T::SIZE)
                .context("encoded row offset overflow")?;
            let end = start
                .checked_add(T::SIZE)
                .context("encoded row range overflow")?;
            let encoded_len = value.try_encode(&mut rows[start..end])?;
            if encoded_len != T::SIZE {
                bail!(
                    "record encoder wrote {encoded_len} bytes, expected {}",
                    T::SIZE
                );
            }
        }

        self.metadata.write_rows(0, &rows)?;
        self.len = new_len;
        Ok(())
    }

    fn validate_existing_index(&self, index: usize) -> Result<()> {
        if index >= self.len {
            bail!(
                "record index {index} is out of bounds for {} records",
                self.len
            );
        }
        Ok(())
    }

    fn shift_rows_left(&mut self, start: usize, end: usize) -> Result<()> {
        if start >= end {
            return Ok(());
        }

        let rows_per_chunk = (SHIFT_BUFFER_BYTES / T::SIZE).max(1);
        let byte_capacity = rows_per_chunk
            .checked_mul(T::SIZE)
            .context("row shift buffer size overflow")?;
        let mut buffer = allocate_bytes(byte_capacity, "row shift buffer")?;
        let mut destination = start;

        while destination < end {
            let rows_to_move = (end - destination).min(rows_per_chunk);
            let source = destination
                .checked_add(1)
                .context("row shift source index overflow")?;
            let byte_len = rows_to_move
                .checked_mul(T::SIZE)
                .context("row shift byte length overflow")?;
            self.metadata
                .read_rows(to_file_index(source)?, &mut buffer[..byte_len])?;
            self.metadata
                .write_rows(to_file_index(destination)?, &buffer[..byte_len])?;
            destination = destination
                .checked_add(rows_to_move)
                .context("row shift destination index overflow")?;
        }
        Ok(())
    }
}

impl<T: SerializedStruct> SerialDataReader<T> for StructFile<T> {
    type Iter<'a>
        = StructFileIter<'a, T>
    where
        Self: 'a,
        T: 'a;

    fn serial_len(&self) -> usize {
        self.len
    }

    fn serial_get(&mut self, index: usize) -> Result<T> {
        self.read_value_at(index)
    }

    fn serial_iter(&mut self) -> Result<Self::Iter<'_>> {
        Ok(StructFileIter {
            reader: BufferedReader::new(&self.metadata)?,
            next_index: 0,
            end_index: self.len,
            _record: PhantomData,
        })
    }
}

impl<T: SerializedStruct> SerialDataWriter<T> for StructFile<T> {
    fn serial_write(&mut self, value: T) -> Result<()> {
        let new_len = self.len.checked_add(1).context("record count overflow")?;
        with_encoded_value(&value, |row| self.metadata.append_row(row))?;
        self.len = new_len;
        Ok(())
    }

    fn serial_insert(&mut self, index: usize, value: T) -> Result<()> {
        if index > self.len {
            bail!(
                "record index {index} is out of bounds for insertion into {} records",
                self.len
            );
        }
        let new_len = self.len.checked_add(1).context("record count overflow")?;
        let file_index = to_file_index(index)?;
        with_encoded_value(&value, |row| self.metadata.insert_row(file_index, row))?;
        self.len = new_len;
        Ok(())
    }

    fn serial_write_at(&mut self, index: usize, value: T) -> Result<()> {
        self.write_value_at(index, &value)
    }

    fn serial_remove(&mut self, index: usize) -> Result<()> {
        self.validate_existing_index(index)?;
        let new_len = self.len - 1;
        self.shift_rows_left(index, new_len)?;
        self.metadata.truncate_rows(to_file_index(new_len)?)?;
        self.len = new_len;
        Ok(())
    }

    fn serial_remove_get(&mut self, index: usize) -> Result<T> {
        self.validate_existing_index(index)?;
        let removed = self.read_value_at(index)?;
        let new_len = self.len - 1;
        self.shift_rows_left(index, new_len)?;
        self.metadata.truncate_rows(to_file_index(new_len)?)?;
        self.len = new_len;
        Ok(removed)
    }

    fn serial_clear(&mut self) -> Result<()> {
        self.metadata.truncate_rows(0)?;
        self.len = 0;
        Ok(())
    }

    fn serial_truncate(&mut self, len: usize) -> Result<()> {
        if len >= self.len {
            return Ok(());
        }
        self.metadata.truncate_rows(to_file_index(len)?)?;
        self.len = len;
        Ok(())
    }
}

impl<T: SerializedStruct> SerialDataContainer<T> for StructFile<T> {}

/// Fallible iterator over typed file records in row order.
pub struct StructFileIter<'file, T: SerializedStruct> {
    reader: BufferedReader<'file>,
    next_index: usize,
    end_index: usize,
    _record: PhantomData<T>,
}

impl<T: SerializedStruct> Iterator for StructFileIter<'_, T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index >= self.end_index {
            return None;
        }

        let index = self.next_index;
        self.next_index += 1;
        let result = match u64::try_from(index) {
            Ok(index) => self.reader.read_row(index).and_then(T::try_decode),
            Err(error) => Err(error.into()),
        };
        if result.is_err() {
            self.next_index = self.end_index;
        }
        Some(result)
    }
}

impl<T: SerializedStruct> std::iter::FusedIterator for StructFileIter<'_, T> {}

fn with_encoded_value<T: SerializedStruct, R>(
    value: &T,
    operation: impl FnOnce(&[u8]) -> Result<R>,
) -> Result<R> {
    if T::SIZE <= STACK_ROW_BUFFER_BYTES {
        let mut row = [0; STACK_ROW_BUFFER_BYTES];
        encode_value_into(value, &mut row[..T::SIZE])?;
        operation(&row[..T::SIZE])
    } else {
        let mut row = allocate_bytes(T::SIZE, "record row")?;
        encode_value_into(value, &mut row)?;
        operation(&row)
    }
}

fn encode_value_into<T: SerializedStruct>(value: &T, row: &mut [u8]) -> Result<()> {
    let encoded_len = value.try_encode(row)?;
    if encoded_len != T::SIZE {
        bail!(
            "record encoder wrote {encoded_len} bytes, expected {}",
            T::SIZE
        );
    }
    Ok(())
}

fn allocate_bytes(length: usize, description: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .with_context(|| format!("failed to allocate {description}"))?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn to_file_index(index: usize) -> Result<u64> {
    u64::try_from(index).context("record index does not fit in file row index")
}

#[cfg(test)]
mod tests {
    use super::allocate_bytes;

    #[test]
    fn reports_row_buffer_allocation_failure() {
        assert!(allocate_bytes(usize::MAX, "test row").is_err());
    }
}
