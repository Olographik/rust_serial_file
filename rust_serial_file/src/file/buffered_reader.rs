use anyhow::{Context, Result, bail};

use super::file_metadata::FileMetadata;

const DEFAULT_CACHE_BYTES: usize = 64 * 1024;

/// A read-only, cached row reader over caller-owned file metadata.
pub struct BufferedReader<'metadata> {
    metadata: &'metadata mut FileMetadata,
    element_size: usize,
    row_count: u64,
    row_capacity: usize,
    row_capacity_u64: u64,
    cache: Vec<u8>,
    cached_start: u64,
    cached_rows: u64,
}

impl<'metadata> BufferedReader<'metadata> {
    /// Creates a reader with a cache targeting 64 KiB of row data.
    pub fn new(metadata: &'metadata mut FileMetadata) -> Result<Self> {
        let element_size =
            usize::try_from(metadata.element_size()).context("row width does not fit in memory")?;

        let row_capacity = (DEFAULT_CACHE_BYTES / element_size).max(1);
        Self::with_capacity_and_width(metadata, row_capacity, element_size)
    }

    /// Creates a reader with an explicit positive row capacity.
    pub fn with_capacity(
        metadata: &'metadata mut FileMetadata,
        row_capacity: usize,
    ) -> Result<Self> {
        let element_size =
            usize::try_from(metadata.element_size()).context("row width does not fit in memory")?;
        Self::with_capacity_and_width(metadata, row_capacity, element_size)
    }

    fn with_capacity_and_width(
        metadata: &'metadata mut FileMetadata,
        row_capacity: usize,
        element_size: usize,
    ) -> Result<Self> {
        if row_capacity == 0 {
            bail!("row capacity must be greater than zero");
        }
        let row_capacity_u64 =
            u64::try_from(row_capacity).context("row capacity does not fit in row index")?;
        let byte_capacity = cache_byte_len(row_capacity, element_size)?;

        let row_count = metadata.element_count()?;
        let cache = allocate_cache(byte_capacity)?;
        Ok(Self {
            metadata,
            element_size,
            row_count,
            row_capacity,
            row_capacity_u64,
            cache,
            cached_start: 0,
            cached_rows: 0,
        })
    }

    /// Changes the row capacity and invalidates the current cache.
    pub fn reset_capacity(&mut self, row_capacity: usize) -> Result<()> {
        if row_capacity == 0 {
            bail!("row capacity must be greater than zero");
        }
        if row_capacity == self.row_capacity {
            return Ok(());
        }

        let row_capacity_u64 =
            u64::try_from(row_capacity).context("row capacity does not fit in row index")?;
        let byte_capacity = cache_byte_len(row_capacity, self.element_size)?;
        if byte_capacity > self.cache.len() {
            self.cache
                .try_reserve_exact(byte_capacity - self.cache.len())
                .context("failed to resize row cache")?;
            self.cache.resize(byte_capacity, 0);
        } else {
            self.cache.truncate(byte_capacity);
        }
        self.row_capacity = row_capacity;
        self.row_capacity_u64 = row_capacity_u64;
        self.cached_rows = 0;
        Ok(())
    }

    /// Returns the row count captured during construction.
    pub fn len(&self) -> u64 {
        self.row_count
    }

    /// Returns whether the captured file contains no complete rows.
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// Returns a row borrowed from the internal cache.
    pub fn read_row(&mut self, index: u64) -> Result<&[u8]> {
        self.ensure_cached(index)?;
        let offset = usize::try_from(index - self.cached_start)
            .context("cached row offset does not fit in memory")?
            .checked_mul(self.element_size)
            .context("cached row byte offset overflow")?;
        let end = offset
            .checked_add(self.element_size)
            .context("cached row byte range overflow")?;
        Ok(&self.cache[offset..end])
    }

    /// Copies a row from the cache into an exact-width caller buffer.
    pub fn read_row_into(&mut self, index: u64, data: &mut [u8]) -> Result<()> {
        if data.len() != self.element_size {
            bail!(
                "row buffer length must equal element_size ({})",
                self.element_size
            );
        }
        data.copy_from_slice(self.read_row(index)?);
        Ok(())
    }

    /// Creates a caller-buffer iterator over all captured rows.
    pub fn iter_rows(&mut self) -> BufferedRowIter<'_, 'metadata> {
        BufferedRowIter::new(self, 0, self.row_count)
    }

    /// Creates a caller-buffer iterator over a validated row range.
    pub fn iter_rows_range(
        &mut self,
        start: u64,
        count: u64,
    ) -> Result<BufferedRowIter<'_, 'metadata>> {
        let end = self.validate_range(start, count)?;
        Ok(BufferedRowIter::new(self, start, end))
    }

    /// Creates an owned-row iterator over all captured rows.
    pub fn rows(&mut self) -> BufferedRows<'_, 'metadata> {
        BufferedRows::new(self, 0, self.row_count)
    }

    /// Creates an owned-row iterator over a validated row range.
    pub fn rows_range(&mut self, start: u64, count: u64) -> Result<BufferedRows<'_, 'metadata>> {
        let end = self.validate_range(start, count)?;
        Ok(BufferedRows::new(self, start, end))
    }

    /// Creates a cache-slice lending iterator over all captured rows.
    pub fn iter_row_slices(&mut self) -> BufferedRowSliceIter<'_, 'metadata> {
        BufferedRowSliceIter::new(self, 0, self.row_count)
    }

    /// Creates a cache-slice lending iterator over a validated row range.
    pub fn iter_row_slices_range(
        &mut self,
        start: u64,
        count: u64,
    ) -> Result<BufferedRowSliceIter<'_, 'metadata>> {
        let end = self.validate_range(start, count)?;
        Ok(BufferedRowSliceIter::new(self, start, end))
    }

    fn ensure_cached(&mut self, index: u64) -> Result<()> {
        if index >= self.row_count {
            bail!(
                "row index {index} is out of bounds for {} captured rows",
                self.row_count
            );
        }

        if index >= self.cached_start && index - self.cached_start < self.cached_rows {
            return Ok(());
        }

        let start = (index / self.row_capacity_u64) * self.row_capacity_u64;
        let row_count = self
            .row_count
            .saturating_sub(start)
            .min(self.row_capacity_u64);
        let byte_count = usize::try_from(row_count)
            .expect("cache row count is bounded by the validated capacity")
            * self.element_size;

        self.cached_rows = 0;
        self.metadata
            .read_rows(start, &mut self.cache[..byte_count])?;
        self.cached_start = start;
        self.cached_rows = row_count;
        Ok(())
    }

    fn validate_range(&self, start: u64, count: u64) -> Result<u64> {
        let end = start.checked_add(count).context("row range overflow")?;
        if end > self.row_count {
            bail!(
                "row range {start}..{end} exceeds {} captured rows",
                self.row_count
            );
        }
        Ok(end)
    }
}

fn cache_byte_len(row_capacity: usize, element_size: usize) -> Result<usize> {
    let byte_capacity = row_capacity
        .checked_mul(element_size)
        .context("cache byte length overflow")?;
    Ok(byte_capacity)
}

fn allocate_cache(byte_capacity: usize) -> Result<Vec<u8>> {
    let mut cache = Vec::new();
    cache
        .try_reserve_exact(byte_capacity)
        .context("failed to allocate row cache")?;
    cache.resize(byte_capacity, 0);
    Ok(cache)
}

/// A caller-buffer iterator over a captured row range.
pub struct BufferedRowIter<'reader, 'metadata> {
    reader: &'reader mut BufferedReader<'metadata>,
    next_index: u64,
    end_index: u64,
    exhausted: bool,
}

impl<'reader, 'metadata> BufferedRowIter<'reader, 'metadata> {
    fn new(reader: &'reader mut BufferedReader<'metadata>, start: u64, end: u64) -> Self {
        Self {
            reader,
            next_index: start,
            end_index: end,
            exhausted: false,
        }
    }

    /// Copies the next row into `data`, returning false after exhaustion.
    pub fn next_into(&mut self, data: &mut [u8]) -> Result<bool> {
        if self.exhausted || self.next_index >= self.end_index {
            self.exhausted = true;
            return Ok(false);
        }

        match self.reader.read_row_into(self.next_index, data) {
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
}

/// An owned-row iterator over a captured row range.
pub struct BufferedRows<'reader, 'metadata> {
    reader: &'reader mut BufferedReader<'metadata>,
    next_index: u64,
    end_index: u64,
    exhausted: bool,
}

impl<'reader, 'metadata> BufferedRows<'reader, 'metadata> {
    fn new(reader: &'reader mut BufferedReader<'metadata>, start: u64, end: u64) -> Self {
        Self {
            reader,
            next_index: start,
            end_index: end,
            exhausted: false,
        }
    }
}

impl Iterator for BufferedRows<'_, '_> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted || self.next_index >= self.end_index {
            self.exhausted = true;
            return None;
        }

        let owned_row = self
            .reader
            .read_row(self.next_index)
            .and_then(try_clone_row);
        if owned_row.is_err() {
            self.exhausted = true;
        } else {
            self.next_index += 1;
        }
        Some(owned_row)
    }
}

impl std::iter::FusedIterator for BufferedRows<'_, '_> {}

fn try_clone_row(row: &[u8]) -> Result<Vec<u8>> {
    let mut owned_row = try_allocate_row(row.len())?;
    owned_row.extend_from_slice(row);
    Ok(owned_row)
}

fn try_allocate_row(row_len: usize) -> Result<Vec<u8>> {
    let mut row = Vec::new();
    row.try_reserve_exact(row_len)
        .context("failed to allocate owned row")?;
    Ok(row)
}

/// A lending iterator yielding slices borrowed from the reader cache.
pub struct BufferedRowSliceIter<'reader, 'metadata> {
    reader: &'reader mut BufferedReader<'metadata>,
    next_index: u64,
    end_index: u64,
    exhausted: bool,
}

impl<'reader, 'metadata> BufferedRowSliceIter<'reader, 'metadata> {
    fn new(reader: &'reader mut BufferedReader<'metadata>, start: u64, end: u64) -> Self {
        Self {
            reader,
            next_index: start,
            end_index: end,
            exhausted: false,
        }
    }

    /// Returns the next borrowed row, or None when the range is exhausted.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Result<&[u8]>> {
        if self.exhausted || self.next_index >= self.end_index {
            self.exhausted = true;
            return None;
        }

        match self.reader.read_row(self.next_index) {
            Ok(row) => {
                self.next_index += 1;
                Some(Ok(row))
            }
            Err(error) => {
                self.exhausted = true;
                Some(Err(error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::try_allocate_row;

    #[test]
    fn reports_owned_row_allocation_overflow() {
        assert!(try_allocate_row(usize::MAX).is_err());
    }
}
