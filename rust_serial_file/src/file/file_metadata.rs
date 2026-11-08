use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};

const COPY_BUFFER_SIZE: usize = 64 * 1024;

/// A fixed-width row view over a file with an optional leading metadata prefix.
pub struct FileMetadata {
    file: File,
    element_size: u64,
    data_offset: u64,
    element_count: u64,
    reserved_rows: u64,
}

impl FileMetadata {
    pub fn new(file: File, element_size: u64, data_offset: u64) -> Result<Self> {
        if element_size == 0 {
            bail!("element_size must be greater than zero");
        }

        let file_len = file.metadata()?.len();
        let element_count = file_len
            .checked_sub(data_offset)
            .map(|data_len| data_len / element_size)
            .unwrap_or(0);

        Ok(Self {
            file,
            element_size,
            data_offset,
            element_count,
            reserved_rows: element_count,
        })
    }

    pub fn open_or_create(
        path: impl AsRef<Path>,
        element_size: u64,
        data_offset: u64,
    ) -> Result<Self> {
        if element_size == 0 {
            bail!("element_size must be greater than zero");
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.as_ref())
            .with_context(|| format!("failed to open or create {}", path.as_ref().display()))?;

        Self::new(file, element_size, data_offset)
    }

    pub fn file_len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    pub fn element_count(&self) -> Result<u64> {
        let file_len = self.file_len()?;
        if file_len == self.row_end(self.reserved_rows)? {
            Ok(self.element_count)
        } else {
            Ok(self.row_count_for_len(file_len))
        }
    }

    pub fn data_offset(&self) -> u64 {
        self.data_offset
    }

    pub fn element_size(&self) -> u64 {
        self.element_size
    }

    pub fn get_raw_metadata(&mut self, metadata: &mut [u8]) -> Result<()> {
        if u64::try_from(metadata.len())? != self.data_offset {
            bail!(
                "metadata buffer length must equal data_offset ({})",
                self.data_offset
            );
        }
        if self.file_len()? < self.data_offset {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file is shorter than its configured metadata prefix",
            )
            .into());
        }

        self.file.seek(SeekFrom::Start(0))?;
        self.file.read_exact(metadata)?;
        Ok(())
    }

    pub fn update_raw_metadata(&mut self, metadata: &[u8]) -> Result<()> {
        self.element_count = self.element_count()?;
        let new_offset = u64::try_from(metadata.len())?;
        let old_len = self.file_len()?;
        let row_region_len = old_len.saturating_sub(self.data_offset);
        let target_len = new_offset
            .checked_add(row_region_len)
            .context("file length overflow while updating metadata")?;

        if new_offset > self.data_offset {
            self.file.set_len(target_len)?;
            move_range(&mut self.file, self.data_offset, new_offset, row_region_len)?;
        } else if new_offset < self.data_offset {
            move_range(&mut self.file, self.data_offset, new_offset, row_region_len)?;
            self.file.set_len(target_len)?;
        }

        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(metadata)?;
        self.data_offset = new_offset;
        Ok(())
    }

    pub fn truncate_rows(&mut self, row_count: u64) -> Result<()> {
        let current_count = self.element_count()?;
        if row_count > current_count {
            bail!("cannot retain {row_count} rows when only {current_count} exist");
        }
        let target_len = self.row_end(row_count)?;
        self.file.set_len(target_len)?;
        self.element_count = row_count;
        self.reserved_rows = row_count;
        Ok(())
    }

    pub fn truncate_rows_front(&mut self, row_count: u64) -> Result<()> {
        let current_count = self.element_count()?;
        if row_count > current_count {
            bail!("cannot retain {row_count} rows when only {current_count} exist");
        }

        let first_retained = current_count - row_count;
        let source = self.row_offset(first_retained)?;
        let target_len = self.row_end(row_count)?;
        let retained_byte_len = self.checked_byte_len(row_count)?;
        move_range(&mut self.file, source, self.data_offset, retained_byte_len)?;
        self.file.set_len(target_len)?;
        self.element_count = row_count;
        self.reserved_rows = row_count;
        Ok(())
    }

    pub fn read_row(&mut self, index: u64, data: &mut [u8]) -> Result<()> {
        if u64::try_from(data.len())? != self.element_size {
            bail!(
                "row buffer length must equal element_size ({})",
                self.element_size
            );
        }
        self.validate_range(index, 1)?;
        self.file.seek(SeekFrom::Start(self.row_offset(index)?))?;
        self.file.read_exact(data)?;
        Ok(())
    }

    pub fn write_row(&mut self, index: u64, row: &[u8]) -> Result<()> {
        if u64::try_from(row.len())? != self.element_size {
            bail!(
                "row buffer length must equal element_size ({})",
                self.element_size
            );
        }
        self.ensure_metadata_prefix()?;
        self.validate_range(index, 1)?;
        self.file.seek(SeekFrom::Start(self.row_offset(index)?))?;
        self.file.write_all(row)?;
        Ok(())
    }

    pub fn read_rows(&mut self, start: u64, data: &mut [u8]) -> Result<()> {
        let row_count = self.block_row_count(data.len())?;
        self.validate_range(start, row_count)?;
        if row_count == 0 {
            return Ok(());
        }

        self.file.seek(SeekFrom::Start(self.row_offset(start)?))?;
        self.file.read_exact(data)?;
        Ok(())
    }

    pub fn write_rows(&mut self, start: u64, data: &[u8]) -> Result<()> {
        let row_count = self.block_row_count(data.len())?;
        if row_count == 0 {
            self.validate_range(start, 0)?;
            return Ok(());
        }

        self.ensure_metadata_prefix()?;
        self.validate_range(start, row_count)?;
        self.file.seek(SeekFrom::Start(self.row_offset(start)?))?;
        self.file.write_all(data)?;
        Ok(())
    }

    pub fn append_row(&mut self, row: &[u8]) -> Result<()> {
        self.validate_row_slice(row)?;
        self.ensure_metadata_prefix()?;

        let count = self.element_count()?;
        let new_count = count
            .checked_add(1)
            .context("row count overflow while appending")?;
        let target_len = self.row_end(new_count)?;
        if new_count > self.reserved_rows {
            self.file.set_len(target_len)?;
            self.reserved_rows = new_count;
        }
        self.file.seek(SeekFrom::Start(self.row_offset(count)?))?;
        self.file.write_all(row)?;
        self.element_count = new_count;
        Ok(())
    }

    pub fn insert_row(&mut self, index: u64, row: &[u8]) -> Result<()> {
        self.validate_row_slice(row)?;
        self.ensure_metadata_prefix()?;

        let count = self.element_count()?;
        if index > count {
            bail!("row index {index} is out of bounds for {count} rows");
        }

        let new_count = count.checked_add(1).context("row count overflow")?;
        let target_len = self.row_end(new_count)?;
        let insert_offset = self.row_offset(index)?;
        let source_end = self.row_end(count)?;

        if new_count > self.reserved_rows {
            self.file.set_len(target_len)?;
            self.reserved_rows = new_count;
        }
        move_range(
            &mut self.file,
            insert_offset,
            insert_offset
                .checked_add(self.element_size)
                .context("insert offset overflow")?,
            source_end - insert_offset,
        )?;
        self.file.seek(SeekFrom::Start(insert_offset))?;
        self.file.write_all(row)?;
        self.element_count = new_count;
        Ok(())
    }

    pub fn reserve_rows(&mut self, new_count: u64) -> Result<()> {
        self.element_count = self.element_count()?;
        if new_count <= self.reserved_rows {
            return Ok(());
        }

        let target_len = self.row_end(new_count)?;
        self.ensure_metadata_prefix()?;
        self.file.set_len(target_len)?;
        self.reserved_rows = new_count;
        Ok(())
    }

    pub fn iter_rows(&mut self) -> Result<FileRowIter<'_>> {
        let row_count = self.element_count()?;
        Ok(FileRowIter {
            metadata: self,
            next_index: 0,
            end_index: row_count,
            exhausted: false,
        })
    }

    pub fn iter_rows_range(&mut self, start: u64, count: u64) -> Result<FileRowIter<'_>> {
        self.validate_range(start, count)?;
        let end_index = start.checked_add(count).context("row range overflow")?;
        Ok(FileRowIter {
            metadata: self,
            next_index: start,
            end_index,
            exhausted: false,
        })
    }

    pub fn rows(&mut self) -> Result<FileRows<'_>> {
        let element_size = usize::try_from(self.element_size)?;
        Ok(FileRows {
            inner: self.iter_rows()?,
            element_size,
            exhausted: false,
        })
    }

    pub fn rows_range(&mut self, start: u64, count: u64) -> Result<FileRows<'_>> {
        let element_size = usize::try_from(self.element_size)?;
        Ok(FileRows {
            inner: self.iter_rows_range(start, count)?,
            element_size,
            exhausted: false,
        })
    }

    fn row_count_for_len(&self, file_len: u64) -> u64 {
        file_len
            .checked_sub(self.data_offset)
            .map(|data_len| data_len / self.element_size)
            .unwrap_or(0)
    }

    fn row_offset(&self, index: u64) -> Result<u64> {
        self.data_offset
            .checked_add(
                index
                    .checked_mul(self.element_size)
                    .context("row offset overflow")?,
            )
            .context("row offset overflow")
    }

    fn row_end(&self, row_count: u64) -> Result<u64> {
        self.data_offset
            .checked_add(self.checked_byte_len(row_count)?)
            .context("file length overflow")
    }

    fn checked_byte_len(&self, row_count: u64) -> Result<u64> {
        row_count
            .checked_mul(self.element_size)
            .context("row byte length overflow")
    }

    fn validate_range(&self, start: u64, count: u64) -> Result<()> {
        let end = start.checked_add(count).context("row range overflow")?;
        let available = self.element_count()?;
        if end > available {
            bail!("row range {start}..{end} exceeds {available} rows");
        }
        Ok(())
    }

    fn block_row_count(&self, byte_len: usize) -> Result<u64> {
        let byte_len = u64::try_from(byte_len)?;
        if byte_len % self.element_size != 0 {
            bail!("block buffer length must be a multiple of element_size");
        }
        Ok(byte_len / self.element_size)
    }

    fn validate_row_slice(&self, row: &[u8]) -> Result<()> {
        if u64::try_from(row.len())? != self.element_size {
            bail!(
                "row buffer length must equal element_size ({})",
                self.element_size
            );
        }
        Ok(())
    }

    fn ensure_metadata_prefix(&mut self) -> Result<()> {
        let file_len = self.file_len()?;
        if file_len < self.data_offset {
            self.file.set_len(self.data_offset)?;
        }
        Ok(())
    }
}

pub struct FileRowIter<'a> {
    metadata: &'a mut FileMetadata,
    next_index: u64,
    end_index: u64,
    exhausted: bool,
}

impl FileRowIter<'_> {
    pub fn next_into(&mut self, data: &mut [u8]) -> Result<bool> {
        if self.exhausted || self.next_index >= self.end_index {
            self.exhausted = true;
            return Ok(false);
        }

        match self.metadata.read_row(self.next_index, data) {
            Ok(()) => {
                self.next_index += 1;
                Ok(true)
            }
            Err(error) => {
                self.exhausted = true;
                Err(error)
            }
        }
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted || self.next_index >= self.end_index
    }
}

pub struct FileRows<'a> {
    inner: FileRowIter<'a>,
    element_size: usize,
    exhausted: bool,
}

impl Iterator for FileRows<'_> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted || self.inner.is_exhausted() {
            self.exhausted = true;
            return None;
        }

        let mut row = Vec::new();
        if let Err(error) = row.try_reserve_exact(self.element_size) {
            self.exhausted = true;
            return Some(Err(error.into()));
        }
        row.resize(self.element_size, 0);

        match self.inner.next_into(&mut row) {
            Ok(true) => Some(Ok(row)),
            Ok(false) => {
                self.exhausted = true;
                None
            }
            Err(error) => {
                self.exhausted = true;
                Some(Err(error))
            }
        }
    }
}

impl std::iter::FusedIterator for FileRows<'_> {}

fn move_range(file: &mut File, source: u64, destination: u64, length: u64) -> Result<()> {
    if length == 0 || source == destination {
        return Ok(());
    }

    let mut buffer = [0; COPY_BUFFER_SIZE];
    if destination > source {
        let mut remaining = length;
        while remaining > 0 {
            let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))?;
            let chunk_offset = remaining - chunk_len as u64;
            file.seek(SeekFrom::Start(source + chunk_offset))?;
            file.read_exact(&mut buffer[..chunk_len])?;
            file.seek(SeekFrom::Start(destination + chunk_offset))?;
            file.write_all(&buffer[..chunk_len])?;
            remaining = chunk_offset;
        }
    } else {
        let mut copied = 0;
        while copied < length {
            let chunk_len = usize::try_from((length - copied).min(buffer.len() as u64))?;
            file.seek(SeekFrom::Start(source + copied))?;
            file.read_exact(&mut buffer[..chunk_len])?;
            file.seek(SeekFrom::Start(destination + copied))?;
            file.write_all(&buffer[..chunk_len])?;
            copied += chunk_len as u64;
        }
    }
    Ok(())
}
