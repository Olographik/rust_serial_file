use anyhow::{Context, Result, bail};

use super::file_metadata::FileMetadata;

const DEFAULT_CACHE_BYTES: usize = 64 * 1024;

/// A buffered, write-only row writer over caller-owned file metadata.
pub struct BufferedWriter<'metadata> {
    metadata: &'metadata mut FileMetadata,
    element_size: usize,
    logical_len: u64,
    row_capacity: usize,
    row_capacity_u64: u64,
    cache: Vec<u8>,
    window_start: u64,
    end_ptr: usize,
}

impl<'metadata> BufferedWriter<'metadata> {
    /// Creates a writer with a cache targeting 64 KiB of row data.
    pub fn new(metadata: &'metadata mut FileMetadata) -> Result<Self> {
        let element_size =
            usize::try_from(metadata.element_size()).context("row width does not fit in memory")?;
        let row_capacity = (DEFAULT_CACHE_BYTES / element_size).max(1);
        Self::with_capacity_and_width(metadata, row_capacity, element_size)
    }

    /// Creates a writer with an explicit positive row capacity.
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
        let byte_capacity = row_capacity
            .checked_mul(element_size)
            .context("cache byte length overflow")?;
        let logical_len = metadata.element_count()?;
        let cache = allocate_cache(byte_capacity)?;

        Ok(Self {
            metadata,
            element_size,
            logical_len,
            row_capacity,
            row_capacity_u64,
            cache,
            window_start: 0,
            end_ptr: 0,
        })
    }

    /// Returns the logical row count, including buffered appends.
    pub fn len(&self) -> u64 {
        self.logical_len
    }

    /// Returns whether the writer contains no logical rows.
    pub fn is_empty(&self) -> bool {
        self.logical_len == 0
    }

    /// Stages an exact-width row overwrite or append.
    pub fn write_row(&mut self, index: u64, row: &[u8]) -> Result<()> {
        if row.len() != self.element_size {
            bail!(
                "row buffer length must equal element_size ({})",
                self.element_size
            );
        }
        self.write_rows(index, row)
    }

    /// Stages a contiguous block, or writes it directly when larger than the cache.
    pub fn write_rows(&mut self, start: u64, rows: &[u8]) -> Result<()> {
        if !rows.len().is_multiple_of(self.element_size) {
            bail!("block byte length must be a multiple of element_size");
        }

        let row_count = u64::try_from(rows.len() / self.element_size)
            .context("block row count does not fit in row index")?;
        let end = start.checked_add(row_count).context("row range overflow")?;
        if start > self.logical_len {
            bail!(
                "row range starts at {start}, beyond the logical end {}",
                self.logical_len
            );
        }
        if row_count == 0 {
            return Ok(());
        }

        if row_count > self.row_capacity_u64 {
            self.flush()?;
            self.metadata.write_rows(start, rows)?;
            self.logical_len = self.logical_len.max(end);
            return Ok(());
        }

        let row_count_usize =
            usize::try_from(row_count).context("block row count does not fit in memory")?;
        self.prepare_window(start, row_count_usize)?;

        let row_offset = usize::try_from(start - self.window_start)
            .context("cache row offset does not fit in memory")?;
        let destination_start = row_offset
            .checked_mul(self.element_size)
            .context("cache byte offset overflow")?;
        let destination_end = destination_start
            .checked_add(rows.len())
            .context("cache byte range overflow")?;
        self.cache[destination_start..destination_end].copy_from_slice(rows);

        self.end_ptr = self.end_ptr.max(row_offset + row_count_usize);
        self.logical_len = self.logical_len.max(end);

        if self.end_ptr == self.row_capacity {
            self.flush()?;
        }
        Ok(())
    }

    /// Writes the contiguous cached range through `FileMetadata`.
    pub fn flush(&mut self) -> Result<()> {
        if self.end_ptr == 0 {
            return Ok(());
        }

        let byte_end = self
            .end_ptr
            .checked_mul(self.element_size)
            .context("cache byte range overflow while flushing")?;
        self.metadata
            .write_rows(self.window_start, &self.cache[..byte_end])?;

        self.end_ptr = 0;
        Ok(())
    }

    fn prepare_window(&mut self, start: u64, row_count: usize) -> Result<()> {
        if self.end_ptr == 0 {
            self.window_start = start;
            return Ok(());
        }

        let fits_active_window = if start < self.window_start {
            false
        } else {
            let offset = usize::try_from(start - self.window_start)
                .context("cache row offset does not fit in memory")?;
            let range_end = offset
                .checked_add(row_count)
                .context("cache row range overflow")?;
            offset <= self.end_ptr && range_end <= self.row_capacity
        };
        if fits_active_window {
            return Ok(());
        }

        self.flush()?;
        self.window_start = start;
        Ok(())
    }
}

impl Drop for BufferedWriter<'_> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

fn allocate_cache(byte_capacity: usize) -> Result<Vec<u8>> {
    let mut cache = Vec::new();
    cache
        .try_reserve_exact(byte_capacity)
        .context("failed to allocate row cache")?;
    cache.resize(byte_capacity, 0);
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::allocate_cache;

    #[test]
    fn reports_cache_allocation_failure() {
        assert!(allocate_cache(usize::MAX).is_err());
    }
}
