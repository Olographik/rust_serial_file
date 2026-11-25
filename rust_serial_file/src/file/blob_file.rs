//! Variable-length values stored outside fixed-width rows.
//!
//! A [`BlobFile`] stores byte values and strings. A record of a struct file
//! points to a value with a [`BlobRef`] (`offset`, `len`), a 12-byte
//! fixed-width field, so the record itself stays fixed-width.
//!
//! # File format
//!
//! All integers are little-endian.
//!
//! ```text
//! file header (16 bytes): magic "RSFBLOB1" | version: u16 | 0: u16 | 0: u32
//! block (repeated)      : state: u32 | capacity: u32 | payload (capacity bytes)
//! ```
//!
//! `state` is 1 for a live block and 0 for a free one. Blocks follow each
//! other from offset 16 to the end of the file. A [`BlobRef`]'s `offset` is
//! the offset of the payload (the block header is the 8 bytes before it) and
//! its `len` is the value length, at most the block's capacity. The length
//! is stored only in the reference.
//!
//! # Free space
//!
//! A value that no longer fits its block moves to another block, and the
//! old block becomes free. Free blocks are reused (best fit, split when the
//! rest is large enough), merged with free neighbours, and dropped from the
//! end of the file. The index of blocks is kept in memory only and rebuilt
//! by scanning the block headers when the file is opened.
//!
//! Holes left between live blocks are removed by compaction, which also
//! rewrites the references in the rows; see
//! [`StructBlobFile::compact`](super::struct_blob_file::StructBlobFile::compact).
//!
//! # Consistency
//!
//! Keeping the rows and the blob file consistent is the caller's job. To
//! update a value without risking a reference to garbage after a crash:
//!
//! 1. write the new value ([`BlobFile::write`]) and [`BlobFile::sync_data`];
//! 2. store the new [`BlobRef`] in the row and sync the struct file;
//! 3. only then [`BlobFile::free`] the old value.
//!
//! A crash can then only leak space, which compaction recovers.
//! [`BlobFile::update`] frees the old block itself and skips that ordering.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

pub use crate::serialized_struct::BlobRef;

use super::file_metadata::read_exact_at;

const MAGIC: [u8; 8] = *b"RSFBLOB1";
const VERSION: u16 = 1;
/// Length of the file header.
pub const FILE_HEADER_LEN: u64 = 16;
/// Length of a block header (`state` and `capacity`).
pub const BLOCK_HEADER_LEN: u64 = 8;
const STATE_FREE: u32 = 0;
const STATE_LIVE: u32 = 1;
/// Capacities are multiples of this, so headers stay 8-byte aligned.
const CAPACITY_ALIGN: u64 = 8;
/// A reused free block is split only if at least this many bytes remain
/// (a block header and 8 payload bytes).
const MIN_SPLIT: u64 = 16;

/// Exposes the [`BlobRef`] fields of a record, so compaction can rewrite
/// them.
///
/// ```
/// use rust_serial_file::file::blob_file::{BlobFields, BlobRef};
///
/// struct Sample {
///     time: u64,
///     name: BlobRef,
///     note: BlobRef,
/// }
///
/// impl BlobFields for Sample {
///     fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef)) {
///         f(&mut self.name);
///         f(&mut self.note);
///     }
/// }
/// ```
pub trait BlobFields {
    /// Calls `f` once with each [`BlobRef`] field of the record.
    fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef));
}

/// Space used by a [`BlobFile`], to decide when to compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlobStats {
    /// Physical file length in bytes.
    pub file_len: u64,
    /// Number of live blocks (stored values).
    pub live_blocks: usize,
    /// Payload capacity of the live blocks, in bytes.
    pub live_bytes: u64,
    /// Number of free blocks.
    pub free_blocks: usize,
    /// Bytes taken by free blocks, headers included: what compaction would
    /// reclaim from free blocks.
    pub free_bytes: u64,
    /// Capacity of the largest free block: the largest value that can be
    /// written without growing the file.
    pub largest_free_block: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Block {
    live: bool,
    capacity: u32,
}

/// A file of variable-length values referenced by [`BlobRef`]s.
///
/// Reads take `&self` and use positioned reads; writes take `&mut self`.
/// See the [module documentation](self) for the format and the update order.
#[derive(Debug)]
pub struct BlobFile {
    file: File,
    path: Option<PathBuf>,
    len: u64,
    /// Every block, by header offset. Used to validate references: a crafted
    /// offset inside a payload must not be taken for a block.
    blocks: BTreeMap<u64, Block>,
    /// Free blocks by (capacity, header offset), for best fit.
    free_by_capacity: BTreeSet<(u32, u64)>,
}

/// Path of the temporary file compaction writes next to `path`
/// (`<path>.compact`).
pub fn compact_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".compact");
    PathBuf::from(name)
}

/// Fails if a compaction of `path` was interrupted (its `.compact` file
/// exists), so the caller can decide how to recover.
pub(crate) fn ensure_no_leftover_compaction(path: &Path) -> Result<()> {
    let leftover = compact_path(path);
    if leftover.exists() {
        bail!(
            "an interrupted compaction left {}; finish or discard it before opening {}",
            leftover.display(),
            path.display()
        );
    }
    Ok(())
}

impl BlobFile {
    /// Opens or creates the blob file at `path`.
    ///
    /// # Errors
    ///
    /// Fails if `<path>.compact` exists (an interrupted compaction), if the
    /// file cannot be opened, or as [`BlobFile::new`].
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure_no_leftover_compaction(path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open or create {}", path.display()))?;
        let mut blob =
            Self::new(file).with_context(|| format!("failed to read {}", path.display()))?;
        blob.path = Some(path.to_owned());
        Ok(blob)
    }

    /// Wraps an opened file: writes the header if the file is empty,
    /// otherwise validates it and indexes its blocks.
    ///
    /// # Errors
    ///
    /// Fails on a bad magic, an unknown version, a non-zero reserved field, a
    /// block state other than 0 or 1, a block running past the end of the
    /// file, trailing bytes shorter than a block header, or an I/O error.
    pub fn new(mut file: File) -> Result<Self> {
        let len = file.metadata()?.len();
        if len == 0 {
            let mut header = [0; FILE_HEADER_LEN as usize];
            header[..8].copy_from_slice(&MAGIC);
            header[8..10].copy_from_slice(&VERSION.to_le_bytes());
            file.write_all(&header)?;
            return Ok(Self::empty(file, FILE_HEADER_LEN));
        }
        if len < FILE_HEADER_LEN {
            bail!("blob file is shorter than its {FILE_HEADER_LEN}-byte header");
        }

        let mut header = [0; FILE_HEADER_LEN as usize];
        read_exact_at(&file, &mut header, 0)?;
        if header[..8] != MAGIC {
            bail!("not a blob file: bad magic");
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != VERSION {
            bail!("unsupported blob file version {version}");
        }
        if header[10..] != [0; 6] {
            bail!("blob file header has non-zero reserved bytes");
        }

        let mut blob = Self::empty(file, len);
        let mut offset = FILE_HEADER_LEN;
        while offset < len {
            if len - offset < BLOCK_HEADER_LEN {
                bail!(
                    "blob file has {} trailing bytes after the last block, fewer than a block header",
                    len - offset
                );
            }
            let block = blob.read_block_header(offset)?;
            let end = offset + BLOCK_HEADER_LEN + u64::from(block.capacity);
            if end > len {
                bail!("block at offset {offset} runs past the end of the blob file");
            }
            blob.insert_block(offset, block);
            offset = end;
        }
        Ok(blob)
    }

    fn empty(file: File, len: u64) -> Self {
        Self {
            file,
            path: None,
            len,
            blocks: BTreeMap::new(),
            free_by_capacity: BTreeSet::new(),
        }
    }

    /// The path the file was opened from, if it was opened by path.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Stores `bytes` and returns its reference. An empty value returns
    /// [`BlobRef::EMPTY`] and uses no space.
    ///
    /// The smallest free block that fits is reused (and split if the rest is
    /// at least 16 bytes); otherwise a block is appended.
    ///
    /// # Errors
    ///
    /// Fails if `bytes` is longer than `u32::MAX - 7` bytes, or on I/O error.
    pub fn write(&mut self, bytes: &[u8]) -> Result<BlobRef> {
        if bytes.is_empty() {
            return Ok(BlobRef::EMPTY);
        }
        let len = u32::try_from(bytes.len()).context("blob value is longer than u32::MAX bytes")?;
        let capacity = aligned_capacity(len)?;

        let header_offset = match self.take_free_block(capacity)? {
            Some(header_offset) => {
                self.write_at(header_offset + BLOCK_HEADER_LEN, bytes)?;
                header_offset
            }
            None => self.append_block(capacity, bytes)?,
        };
        Ok(BlobRef::new(header_offset + BLOCK_HEADER_LEN, len))
    }

    /// Stores a string as UTF-8 bytes. See [`write`](Self::write).
    ///
    /// # Errors
    ///
    /// As [`write`](Self::write).
    pub fn write_str(&mut self, value: &str) -> Result<BlobRef> {
        self.write(value.as_bytes())
    }

    /// Reads the value of `reference`.
    ///
    /// # Errors
    ///
    /// Fails if `reference` does not designate a live block whose capacity
    /// is at least `reference.len`, if the buffer cannot be allocated, or on
    /// I/O error.
    pub fn read(&self, reference: BlobRef) -> Result<Vec<u8>> {
        let len = self.checked_len(reference)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .context("failed to allocate the blob value buffer")?;
        bytes.resize(len, 0);
        self.read_payload(reference, &mut bytes)?;
        Ok(bytes)
    }

    /// Reads the value of `reference` into `buffer`, whose length must be
    /// `reference.len`.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read), and if `buffer` has another length.
    pub fn read_into(&self, reference: BlobRef, buffer: &mut [u8]) -> Result<()> {
        let len = self.checked_len(reference)?;
        if buffer.len() != len {
            bail!(
                "buffer length {} does not match the blob value length {len}",
                buffer.len()
            );
        }
        self.read_payload(reference, buffer)
    }

    /// Reads the value of `reference` as a UTF-8 string.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read), and if the bytes are not valid UTF-8.
    pub fn read_string(&self, reference: BlobRef) -> Result<String> {
        String::from_utf8(self.read(reference)?).context("blob value is not valid UTF-8")
    }

    /// Replaces the value of `reference` and returns the reference to store
    /// in the row.
    ///
    /// The value is overwritten in place when it fits the block's capacity
    /// (same `offset`). Otherwise it is written to another block (see
    /// [`write`](Self::write)) and the old block is freed. Replacing with an
    /// empty value frees the block and returns [`BlobRef::EMPTY`]; replacing
    /// [`BlobRef::EMPTY`] writes a new value.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read) for an invalid `reference` (nothing is
    /// written), and as [`write`](Self::write).
    pub fn update(&mut self, reference: BlobRef, bytes: &[u8]) -> Result<BlobRef> {
        if reference.is_empty() {
            return self.write(bytes);
        }
        let header_offset = self.live_block(reference)?.0;
        if bytes.is_empty() {
            self.free(reference)?;
            return Ok(BlobRef::EMPTY);
        }
        let capacity = self.blocks[&header_offset].capacity;
        if let Ok(len) = u32::try_from(bytes.len())
            && len <= capacity
        {
            self.write_at(reference.offset, bytes)?;
            return Ok(BlobRef::new(reference.offset, len));
        }
        let moved = self.write(bytes)?;
        self.free(reference)?;
        Ok(moved)
    }

    /// Frees the block of `reference`, merging it with free neighbours. A
    /// free block at the end of the file is removed by truncating the file.
    /// Freeing [`BlobRef::EMPTY`] does nothing.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read) for an invalid `reference` (nothing is
    /// changed), or on I/O error.
    pub fn free(&mut self, reference: BlobRef) -> Result<()> {
        if reference.is_empty() {
            return Ok(());
        }
        let (freed_offset, block) = self.live_block(reference)?;
        // Work out the merged free block first; the index is only changed
        // once the file write succeeded.
        let mut header_offset = freed_offset;
        let mut capacity = u64::from(block.capacity);
        let mut merged_blocks = vec![freed_offset];

        let next_offset = header_offset + BLOCK_HEADER_LEN + capacity;
        if let Some(next) = self.blocks.get(&next_offset)
            && !next.live
            && let Some(merged) = merged_capacity(capacity, next.capacity)
        {
            merged_blocks.push(next_offset);
            capacity = merged;
        }
        if let Some((&previous_offset, previous)) = self.blocks.range(..header_offset).next_back()
            && !previous.live
            && previous_offset + BLOCK_HEADER_LEN + u64::from(previous.capacity) == header_offset
            && let Some(merged) = merged_capacity(capacity, previous.capacity)
        {
            merged_blocks.push(previous_offset);
            header_offset = previous_offset;
            capacity = merged;
        }

        if header_offset + BLOCK_HEADER_LEN + capacity == self.len {
            self.truncate_to(header_offset)?;
            for offset in merged_blocks {
                self.remove_block(offset);
            }
            return self.drop_free_tail();
        }
        let block = Block {
            live: false,
            capacity: u32::try_from(capacity).expect("merged capacities are checked to fit u32"),
        };
        self.write_block_header(header_offset, block)?;
        for offset in merged_blocks {
            self.remove_block(offset);
        }
        self.insert_block(header_offset, block);
        Ok(())
    }

    /// Reports the space used by the file.
    pub fn stats(&self) -> BlobStats {
        let mut stats = BlobStats {
            file_len: self.len,
            ..BlobStats::default()
        };
        for block in self.blocks.values() {
            if block.live {
                stats.live_blocks += 1;
                stats.live_bytes += u64::from(block.capacity);
            } else {
                stats.free_blocks += 1;
                stats.free_bytes += BLOCK_HEADER_LEN + u64::from(block.capacity);
                stats.largest_free_block = stats.largest_free_block.max(block.capacity);
            }
        }
        stats
    }

    /// Makes the file's content and length durable (`File::sync_data`).
    ///
    /// # Errors
    ///
    /// Returns the sync error; after a failure, treat earlier writes as
    /// possibly not persisted.
    pub fn sync_data(&mut self) -> Result<()> {
        self.file
            .sync_data()
            .context("failed to sync blob file data to storage")
    }

    /// Like [`sync_data`](Self::sync_data), also syncing file metadata
    /// (`File::sync_all`).
    ///
    /// # Errors
    ///
    /// As [`sync_data`](Self::sync_data).
    pub fn sync_all(&mut self) -> Result<()> {
        self.file
            .sync_all()
            .context("failed to sync blob file data and metadata to storage")
    }

    /// Validates `reference` and returns its length as `usize`; 0 for an
    /// empty reference.
    fn checked_len(&self, reference: BlobRef) -> Result<usize> {
        if reference.is_empty() {
            return Ok(0);
        }
        self.live_block(reference)?;
        usize::try_from(reference.len).context("blob value length does not fit in memory")
    }

    /// Returns the live block designated by a non-empty `reference`.
    fn live_block(&self, reference: BlobRef) -> Result<(u64, Block)> {
        let header_offset = reference
            .offset
            .checked_sub(BLOCK_HEADER_LEN)
            .filter(|offset| *offset >= FILE_HEADER_LEN);
        let Some(header_offset) = header_offset else {
            bail!(
                "invalid blob reference: offset {} is not a block payload",
                reference.offset
            );
        };
        match self.blocks.get(&header_offset) {
            Some(block) if block.live && reference.len <= block.capacity => {
                Ok((header_offset, *block))
            }
            Some(block) if block.live => bail!(
                "invalid blob reference: length {} exceeds the block capacity {}",
                reference.len,
                block.capacity
            ),
            Some(_) => bail!(
                "invalid blob reference: the block at offset {} is free",
                reference.offset
            ),
            None => bail!(
                "invalid blob reference: offset {} is not a block payload",
                reference.offset
            ),
        }
    }

    fn read_payload(&self, reference: BlobRef, buffer: &mut [u8]) -> Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }
        read_exact_at(&self.file, buffer, reference.offset).context("failed to read a blob value")
    }

    /// Takes the smallest free block of at least `capacity`, marks it live
    /// (splitting off the rest when large enough) and returns its header
    /// offset.
    fn take_free_block(&mut self, capacity: u32) -> Result<Option<u64>> {
        let Some(&(free_capacity, header_offset)) =
            self.free_by_capacity.range((capacity, 0)..).next()
        else {
            return Ok(None);
        };
        let rest = u64::from(free_capacity) - u64::from(capacity);
        let split = (rest >= MIN_SPLIT).then(|| {
            let rest_offset = header_offset + BLOCK_HEADER_LEN + u64::from(capacity);
            let rest_block = Block {
                live: false,
                capacity: u32::try_from(rest - BLOCK_HEADER_LEN)
                    .expect("the rest is smaller than the free block"),
            };
            (rest_offset, rest_block)
        });
        let block = Block {
            live: true,
            capacity: if split.is_some() {
                capacity
            } else {
                free_capacity
            },
        };
        // File first (rest header, then the live header), index after.
        if let Some((rest_offset, rest_block)) = split {
            self.write_block_header(rest_offset, rest_block)?;
        }
        self.write_block_header(header_offset, block)?;
        self.remove_block(header_offset);
        self.insert_block(header_offset, block);
        if let Some((rest_offset, rest_block)) = split {
            self.insert_block(rest_offset, rest_block);
        }
        Ok(Some(header_offset))
    }

    /// Appends a live block of `capacity` holding `bytes` (zero padded) and
    /// returns its header offset.
    fn append_block(&mut self, capacity: u32, bytes: &[u8]) -> Result<u64> {
        let header_offset = self.len;
        let block_len = BLOCK_HEADER_LEN + u64::from(capacity);
        let buffer_len = usize::try_from(block_len).context("blob block does not fit in memory")?;
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(buffer_len)
            .context("failed to allocate the blob block buffer")?;
        buffer.extend_from_slice(&STATE_LIVE.to_le_bytes());
        buffer.extend_from_slice(&capacity.to_le_bytes());
        buffer.extend_from_slice(bytes);
        buffer.resize(buffer_len, 0);
        self.write_at(header_offset, &buffer)?;
        self.len = header_offset
            .checked_add(block_len)
            .context("blob file length overflow")?;
        self.insert_block(
            header_offset,
            Block {
                live: true,
                capacity,
            },
        );
        Ok(header_offset)
    }

    /// Removes free blocks left at the end of the file (only possible when
    /// a file was opened with adjacent free blocks).
    fn drop_free_tail(&mut self) -> Result<()> {
        while let Some((&header_offset, &block)) = self.blocks.last_key_value() {
            if block.live {
                break;
            }
            self.remove_block(header_offset);
            self.truncate_to(header_offset)?;
        }
        Ok(())
    }

    fn truncate_to(&mut self, len: u64) -> Result<()> {
        self.file
            .set_len(len)
            .context("failed to truncate the blob file")?;
        self.len = len;
        Ok(())
    }

    fn insert_block(&mut self, header_offset: u64, block: Block) {
        if !block.live {
            self.free_by_capacity
                .insert((block.capacity, header_offset));
        }
        self.blocks.insert(header_offset, block);
    }

    fn remove_block(&mut self, header_offset: u64) {
        if let Some(block) = self.blocks.remove(&header_offset)
            && !block.live
        {
            self.free_by_capacity
                .remove(&(block.capacity, header_offset));
        }
    }

    fn read_block_header(&self, header_offset: u64) -> Result<Block> {
        let mut header = [0; BLOCK_HEADER_LEN as usize];
        read_exact_at(&self.file, &mut header, header_offset)?;
        let state = u32::from_le_bytes(header[..4].try_into().expect("4 bytes"));
        let capacity = u32::from_le_bytes(header[4..].try_into().expect("4 bytes"));
        let live = match state {
            STATE_LIVE => true,
            STATE_FREE => false,
            _ => bail!("block at offset {header_offset} has an invalid state {state}"),
        };
        Ok(Block { live, capacity })
    }

    fn write_block_header(&mut self, header_offset: u64, block: Block) -> Result<()> {
        let state = if block.live { STATE_LIVE } else { STATE_FREE };
        let mut header = [0; BLOCK_HEADER_LEN as usize];
        header[..4].copy_from_slice(&state.to_le_bytes());
        header[4..].copy_from_slice(&block.capacity.to_le_bytes());
        self.write_at(header_offset, &header)
    }

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file
            .write_all(bytes)
            .context("failed to write to the blob file")
    }
}

/// `len` rounded up to a multiple of 8.
fn aligned_capacity(len: u32) -> Result<u32> {
    let aligned = u64::from(len).div_ceil(CAPACITY_ALIGN) * CAPACITY_ALIGN;
    u32::try_from(aligned)
        .context("blob value is too long to be stored (at most u32::MAX - 7 bytes)")
}

/// Capacity of two adjacent blocks merged into one, if it fits a `u32`.
fn merged_capacity(capacity: u64, other: u32) -> Option<u64> {
    let merged = capacity + BLOCK_HEADER_LEN + u64::from(other);
    (merged <= u64::from(u32::MAX)).then_some(merged)
}

#[cfg(test)]
mod tests {
    use super::{aligned_capacity, merged_capacity};

    #[test]
    fn capacities_are_aligned_and_bounded() {
        assert_eq!(aligned_capacity(1).unwrap(), 8);
        assert_eq!(aligned_capacity(8).unwrap(), 8);
        assert_eq!(aligned_capacity(9).unwrap(), 16);
        assert_eq!(aligned_capacity(u32::MAX - 7).unwrap(), u32::MAX - 7);
        assert!(aligned_capacity(u32::MAX - 6).is_err());
        assert_eq!(merged_capacity(8, 8), Some(24));
        assert_eq!(
            merged_capacity(u64::from(u32::MAX) - 16, 8),
            Some(u64::from(u32::MAX))
        );
        assert_eq!(merged_capacity(u64::from(u32::MAX) - 15, 8), None);
    }
}
