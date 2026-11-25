# File Metadata Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.8
- **Short description:** Define a non-generic owned-file wrapper for metadata and unbuffered fixed-size row operations.

## Table of contents

- [Overview](#overview)
- [Goals and scope](#goals-and-scope)
- [Assumptions and dependencies](#assumptions-and-dependencies)
- [Requirements](#requirements)
- [Public API](#public-api)
- [Errors and failure behavior](#errors-and-failure-behavior)
- [Safety and security](#safety-and-security)
- [Compatibility](#compatibility)
- [Performance and resource use](#performance-and-resource-use)
- [Phases](#phases)
    - [Phase planning and reflection](#phase-planning-and-reflection)
    - [Current implementation checklist](#current-implementation-checklist)
- [Testing and acceptance criteria](#testing-and-acceptance-criteria)
- [Code coverage](#code-coverage)
- [Out of scope](#out-of-scope)

## Overview

Specify a file metadata wrapper inspired by the legacy
`rust_serial_file/src/file_old/mod.rs` `FileReference`. The wrapper describes a
file as a sequence of fixed-size elements and reports the physical file length
and number of complete elements in the data region.

The wrapper is independent of any element struct or type `T`. An optional
offset marks where the data region begins and is also the raw metadata length:
the first `data_offset` bytes belong to metadata, followed by fixed-size rows.
The wrapper can retrieve or replace those metadata bytes, move the row region
if metadata changes size, truncate rows from either end, and read or write
individual rows or contiguous blocks of rows using caller-provided buffers.
It can also append a row or insert one before an existing row, shifting later
row data to preserve its order, and reserve storage for a requested number of
rows.
It takes ownership of a `File`. Read operations use cursor-independent
positioned reads and shared references, allowing concurrent readers. The
wrapper does not provide synchronization; callers must coordinate writes and
structural mutations exclusively.

## Goals and scope

### Goals

- Provide the current file length in bytes.
- Provide the number of complete fixed-size elements in the data region.
- Represent the fixed element width and data offset as runtime metadata, not
  as generic type parameters.
- Support a leading in-file metadata area through an explicit data offset.
- Open the wrapper from a path and create the file when it does not exist.
- Own the `File` passed to the wrapper without introducing reference-counted
  sharing or clone behavior.
- Retrieve and replace raw metadata while preserving row contents if the
  metadata size changes.
- Truncate rows from the back or front without changing metadata.
- Read and write individual rows directly, without a buffered row API.
- Read and write contiguous blocks of rows into or from caller-provided
  buffers, without per-row output allocations.
- Append a row after the existing complete rows and insert a row at a selected
  row index while preserving the order and bytes of later row data.
- Extend the file length to reserve capacity for a requested row count,
  without increasing the current instance's logical row count.
- Iterate read-only over all rows or a selected row range using a
  caller-provided reusable buffer.
- Allow read-only operations to run concurrently without changing or sharing
  the file cursor.
- Provide conventional Rust iterator APIs for all rows and selected ranges,
  yielding fallible owned row values.

### Scope

- Specify a non-generic file metadata wrapper and its length/count queries.
- Specify path-based opening that creates a missing file without truncating
  an existing one.
- Define the meaning of the element size and data offset, including boundary
  behavior.
- Define ownership of the file handle, concurrent read behavior, and the
  caller's responsibility to coordinate writes and additional handles.
- Define raw metadata access, metadata resizing, row truncation, unbuffered
  single-row and block row I/O, append, insertion, and row-capacity reservation.
- Define allocation-free read-only iteration over the full row set and a
  selected row range.
- Define standard `Iterator` implementations for row iteration, including
  their owned-item allocation and error semantics.
- Use the legacy `FileReference` only as behavioral inspiration; this
  requirement does not re-enable or preserve its generic API.
- Do not specify element serialization or provide a read/write lock.

## Assumptions and dependencies

- The wrapper takes ownership of a `std::fs::File`.
- The wrapper does not implement `Clone` and does not use `Arc` or `Rc` to
  share the file or metadata configuration.
- Callers that need multiple handles may create them before passing a handle
  to the wrapper; any coordination between those handles is the caller's
  responsibility.
- The wrapper is not generic over a record type. Element width is supplied as
  a nonzero runtime value.
- Positioned reads are available on Unix and Windows and do not modify the
  shared file cursor. The wrapper does not provide synchronization for writes
  or structural mutations.
- The data offset is a byte offset from the beginning of the file. Bytes
  before it are the raw metadata region; its value is therefore the metadata
  byte length and the start offset of the row region.
- Metadata and rows are opaque bytes. The wrapper does not know their
  application-level format.
- Metadata and row reads fill caller-provided mutable byte slices. The caller
  chooses and owns the buffers, so these read methods do not require a heap
  allocation.
- Row capacity reservation extends logical file length only; it does not
  guarantee physical disk-block allocation and may create a sparse extension.
- Reserved capacity and logical row count are tracked only by the current
  wrapper instance. Reopening derives the row count from file length, so
  previously reserved rows then appear as logical rows.
- File length is queried from the file handle when requested and is not a
  cached snapshot.
- Existing project Rust edition, MSRV, and dependency policies apply.

## Requirements

### Functional requirements

| ID                | Requirement                                                                                                                                                                                                                       |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RSF-FILE-FR-001` | The component shall provide a public file metadata wrapper that reports the underlying file's current physical length in bytes.                                                                                                   |
| `RSF-FILE-FR-002` | The wrapper shall describe file contents as a sequence of fixed-size elements using a runtime element size greater than zero.                                                                                                     |
| `RSF-FILE-FR-003` | The wrapper shall report the number of complete elements in the data region, excluding any bytes before the configured data offset.                                                                                               |
| `RSF-FILE-FR-004` | The wrapper shall allow a caller to configure a data offset marking the start of the row region; the first `data_offset` bytes are the in-file metadata.                                                                          |
| `RSF-FILE-FR-005` | The wrapper shall not require, store, or be parameterized by an element struct or generic type `T`.                                                                                                                               |
| `RSF-FILE-FR-006` | The wrapper shall take ownership of the `File` passed to it and shall not introduce reference-counted sharing.                                                                                                                    |
| `RSF-FILE-FR-007` | The wrapper shall not implement `Clone`; callers are responsible for creating and coordinating any additional file handles.                                                                                                       |
| `RSF-FILE-FR-008` | Length and element-count queries shall observe the current file length when called, including changes made through caller-managed handles or external operations.                                                                 |
| `RSF-FILE-FR-009` | The component shall provide a path-based constructor that opens an existing file or creates it when it does not exist. Opening an existing file shall preserve its contents.                                                      |
| `RSF-FILE-FR-010` | The component shall provide `get_raw_metadata`, filling a caller-provided buffer with exactly the first `data_offset` bytes.                                                                                                      |
| `RSF-FILE-FR-011` | The component shall provide a metadata update operation that replaces the raw metadata and preserves all row-region bytes; if metadata length changes, the row region shall be moved to begin immediately after the new metadata. |
| `RSF-FILE-FR-012` | The component shall provide `truncate_rows(row_count)`, retaining the first `row_count` rows and removing rows after them without modifying metadata.                                                                             |
| `RSF-FILE-FR-013` | The component shall provide `truncate_rows_front(row_count)`, removing rows from the beginning and retaining exactly the last `row_count` rows, without modifying metadata.                                                       |
| `RSF-FILE-FR-014` | The component shall provide unbuffered row reads and writes addressed by row index; row payloads shall be raw bytes of exactly the configured element size.                                                                       |
| `RSF-FILE-FR-015` | The component shall expose read-only `data_offset()` and `element_size()` accessors for the corresponding configuration values.                                                                                                   |
| `RSF-FILE-FR-016` | The component shall provide read-only iteration over all complete rows and over an explicitly selected row range, filling a caller-provided reusable buffer for each row.                                                         |
| `RSF-FILE-FR-017` | The component shall provide standard Rust iterators over all complete rows and over an explicitly selected row range, yielding each row as a fallible owned byte vector.                                                          |
| `RSF-FILE-FR-018` | The component shall provide `read_rows` to read a contiguous range of complete rows into an exact-size caller-provided byte buffer.                                                                                               |
| `RSF-FILE-FR-019` | The component shall provide `write_rows` to overwrite a contiguous range of complete rows and extend the logical row count when the range contiguously reaches beyond the current end.                                            |
| `RSF-FILE-FR-020` | The component shall provide `append_row` to add one row after the existing complete rows.                                                                                                                                         |
| `RSF-FILE-FR-021` | The component shall provide `insert_row` to insert one row before the existing row at `index`, shifting subsequent row data to preserve it. Insertion at the row count shall append.                                              |
| `RSF-FILE-FR-022` | Non-empty row-writing, append, and insertion operations shall ensure the metadata prefix exists by zero-filling missing bytes up to `data_offset`, without changing any existing prefix bytes.                                    |
| `RSF-FILE-FR-023` | The component shall provide `reserve_rows(new_count)` to extend the file to hold at least `new_count` rows without increasing the current instance's logical row count.                                                           |

### Data and serialization

| ID                | Requirement                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RSF-FILE-DS-001` | The reported file length shall be the physical length in bytes, including any leading metadata and any trailing incomplete element bytes.                                                                                                                                                                                                                                                                                                                                                                   |
| `RSF-FILE-DS-002` | On construction, the logical element count shall be initialized to `floor((file_len - data_offset) / element_size)` when `file_len` is greater than `data_offset`, and zero otherwise. The current instance's count shall then be updated by its row mutations and shall not count reserved rows until they are appended or inserted. A newly constructed wrapper treats all complete rows implied by file length as logical rows; reserved capacity is not persisted.                                      |
| `RSF-FILE-DS-003` | Bytes remaining after the last complete element shall be treated as invalid trailing data and shall not contribute to the element count or be returned as a row. Read/count operations shall leave these bytes unchanged.                                                                                                                                                                                                                                                                                   |
| `RSF-FILE-DS-004` | Element size and data offset shall be interpreted as byte counts and shall not depend on the layout or size of a Rust type.                                                                                                                                                                                                                                                                                                                                                                                 |
| `RSF-FILE-DS-005` | The first `data_offset` bytes shall be the complete raw metadata region. Metadata length is exactly `data_offset`; metadata updates set the new offset to the new metadata byte length.                                                                                                                                                                                                                                                                                                                     |
| `RSF-FILE-DS-006` | Updating metadata shall preserve the byte sequence of the entire row region, including any trailing incomplete-row bytes, while shifting it by the metadata-length difference.                                                                                                                                                                                                                                                                                                                              |
| `RSF-FILE-DS-007` | `truncate_rows(n)` shall retain rows `[0, n)`; `truncate_rows_front(n)` shall retain the last `n` rows, compacted to begin at `data_offset`. Both operations shall discard trailing incomplete-row bytes and shall leave the metadata bytes unchanged.                                                                                                                                                                                                                                                      |
| `RSF-FILE-DS-008` | A row read or single-row write shall address byte range `[data_offset + index * element_size, data_offset + (index + 1) * element_size)`. Reads and `write_row` shall require an existing complete row. `write_rows` may overwrite and contiguously extend rows when `start <= element_count`, but shall reject gaps when `start > element_count`; the metadata prefix may be padded as specified by `RSF-FILE-DS-015`. |
| `RSF-FILE-DS-009` | Full iteration shall visit complete rows in ascending index order from zero through the row count captured when the iterator is created. Range iteration shall visit rows `[start, start + count)` in ascending order, with the range validated against the row count at iterator creation.                                                                                                                                                                                                                 |
| `RSF-FILE-DS-010` | Standard iterator items shall be `anyhow::Result<Vec<u8>>`; each successful item shall contain exactly one complete row. The iterator shall end after its captured row range and shall report row read failures as `Err` items.                                                                                                                                                                                                                                                                             |
| `RSF-FILE-DS-011` | A block buffer shall contain a concatenation of complete rows in ascending file order. Its byte length shall be an exact multiple of `element_size`; the number of rows is the buffer length divided by `element_size`.                                                                                                                                                                                                                                                                                     |
| `RSF-FILE-DS-012` | A block operation shall address rows `[start, start + block_row_count)`. It shall reject overflowing ranges before file mutation. A non-empty `write_rows` range may overwrite existing rows and extend beyond the current logical end only when `start` is at or before that end; it shall reject a start beyond the end to prevent gaps, and shall update the logical count after a successful extension. A non-empty block write shall first ensure the metadata prefix exists as specified by `RSF-FILE-DS-015`; an empty buffer shall be a no-op when `start` is at most the current row count. |
| `RSF-FILE-DS-013` | `append_row` shall write exactly one complete row at the byte position immediately after the existing logical rows. It shall overwrite any incomplete trailing bytes. If the file has no reserved capacity for the new row, it shall extend the file to the row's end; otherwise it shall not change the file length.                                                                                                                                                                                       |
| `RSF-FILE-DS-014` | `insert_row(index, row)` shall place the supplied row before the logical row at `index`, shift subsequent logical rows by one element width, and discard any incomplete trailing bytes. The valid index range is `0..=element_count`; inserting at `element_count` is equivalent to appending. It shall extend the file only when the new logical row count exceeds reserved capacity.                                                                                                                      |
| `RSF-FILE-DS-015` | Before a non-empty row write, append, or insertion proceeds, if the physical file is shorter than `data_offset`, the operation shall extend it to `data_offset` with zero bytes without changing any existing file bytes.                                                                                                                                                                                                                                                                                   |
| `RSF-FILE-DS-016` | When `new_count` exceeds the reserved capacity, `reserve_rows` shall extend the file length to `data_offset + new_count * element_size` using `File::set_len`; the newly extended bytes shall read as zero. It shall not change the current instance's logical row count. Existing bytes, including any incomplete trailing suffix, shall remain at their original offsets. Reserved capacity is instance-local: after reopening, all complete rows implied by the file length are treated as logical rows. |

### Rust-specific requirements

| ID                | Requirement                                                                                                                                                                                                                 |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RSF-FILE-RS-001` | The public wrapper shall be a concrete, non-generic type.                                                                                                                                                                   |
| `RSF-FILE-RS-002` | The wrapper shall own a concrete `std::fs::File`. Handle-based construction shall take `File` by value; path-based construction shall store the opened or created `File` directly.                                          |
| `RSF-FILE-RS-003` | The wrapper shall reject a zero element size at construction.                                                                                                                                                               |
| `RSF-FILE-RS-004` | File metadata queries shall return observable errors when the operating system cannot retrieve file metadata; they shall not substitute a length of zero.                                                                   |
| `RSF-FILE-RS-005` | The wrapper shall not provide a lock; callers synchronize writes and mutations, while positioned reads may run concurrently without sharing the file cursor.                                                                     |
| `RSF-FILE-RS-006` | `get_raw_metadata` and row reads shall fill caller-provided mutable slices and shall not allocate an output `Vec<u8>`. The buffer length shall exactly match the metadata length or configured element size, respectively. |
| `RSF-FILE-RS-007` | Row read/write methods shall perform direct, unbuffered row operations and shall not add a row buffer to the wrapper.                                                                                                       |
| `RSF-FILE-RS-008` | Row write input shall be validated to have exactly `element_size` bytes; invalid row sizes and out-of-range row indices shall return errors.                                                                                |
| `RSF-FILE-RS-009` | Both row truncation methods shall reject a requested retained row count greater than the current logical row count and shall not extend or pad the file.                                                                    |
| `RSF-FILE-RS-010` | The wrapper shall store `data_offset` as a plain `u64` and shall not implement `Clone` or internally synchronize access.                                                                                                    |
| `RSF-FILE-RS-011` | The file handle, layout fields, logical row count, and reserved capacity shall remain private. The file handle shall not be exposed; element size and offset shall be observable only through immutable getters.            |
| `RSF-FILE-RS-012` | The allocation-avoiding row iteration API shall use a custom buffer-filling iterator; each step shall fill an exact-size caller-provided buffer and shall not allocate an output row.                                       |
| `RSF-FILE-RS-013` | The conventional row iterator shall implement `std::iter::Iterator<Item = anyhow::Result<Vec<u8>>>`; allocation of an owned row value per successful item is accepted for this API.                                         |
| `RSF-FILE-RS-014` | `read_rows` and `write_rows` shall use caller-provided byte slices, validate that the byte length contains only whole rows, and shall not allocate an output buffer.                                                        |
| `RSF-FILE-RS-015` | `append_row` and `insert_row` shall accept a caller-provided slice of exactly `element_size` bytes; insertion shall not require the caller to provide or clone the rows being shifted.                                      |
| `RSF-FILE-RS-016` | `write_row` shall remain replacement-only for complete existing rows. `write_rows` shall allow contiguous extension but reject a starting index beyond the logical end; non-empty writes shall ensure the metadata prefix exists before range validation. |
| `RSF-FILE-RS-017` | `reserve_rows` shall use `File::set_len` to extend file length and shall not claim to guarantee physical disk-block allocation.                                                                                             |
| `RSF-FILE-RS-018` | Metadata and row reads shall use cursor-independent positioned reads on Unix and Windows, so concurrent shared reads do not race on the file cursor.                                                                           |

## Public API

The public type should be named `FileMetadata` and should keep its
configuration fields private so the nonzero element-size invariant cannot be
bypassed. A representative API is:

```rust
use std::fs::File;

pub struct FileMetadata {
    file: File,
    element_size: u64,
    data_offset: u64,
}

impl FileMetadata {
    pub fn new(
        file: File,
        element_size: u64,
        data_offset: u64,
    ) -> anyhow::Result<Self>;

    pub fn open_or_create(
        path: impl AsRef<std::path::Path>,
        element_size: u64,
        data_offset: u64,
    ) -> anyhow::Result<Self>;

    pub fn file_len(&self) -> anyhow::Result<u64>;

    pub fn element_count(&self) -> anyhow::Result<u64>;

    pub fn data_offset(&self) -> u64;

    pub fn element_size(&self) -> u64;

    pub fn get_raw_metadata(&self, metadata: &mut [u8]) -> anyhow::Result<()>;

    pub fn update_raw_metadata(&mut self, metadata: &[u8]) -> anyhow::Result<()>;

    pub fn truncate_rows(&mut self, row_count: u64) -> anyhow::Result<()>;

    pub fn truncate_rows_front(&mut self, row_count: u64) -> anyhow::Result<()>;

    pub fn read_row(&self, index: u64, data: &mut [u8]) -> anyhow::Result<()>;

    pub fn write_row(&mut self, index: u64, row: &[u8]) -> anyhow::Result<()>;

    pub fn read_rows(&self, start: u64, data: &mut [u8]) -> anyhow::Result<()>;

    pub fn write_rows(&mut self, start: u64, data: &[u8]) -> anyhow::Result<()>;

    pub fn append_row(&mut self, row: &[u8]) -> anyhow::Result<()>;

    pub fn insert_row(&mut self, index: u64, row: &[u8]) -> anyhow::Result<()>;

    pub fn reserve_rows(&mut self, new_count: u64) -> anyhow::Result<()>;

    pub fn iter_rows(&self) -> anyhow::Result<FileRowIter<'_>>;

    pub fn iter_rows_range(
        &self,
        start: u64,
        count: u64,
    ) -> anyhow::Result<FileRowIter<'_>>;

    pub fn rows(&self) -> anyhow::Result<FileRows<'_>>;

    pub fn rows_range(
        &self,
        start: u64,
        count: u64,
    ) -> anyhow::Result<FileRows<'_>>;
}

pub struct FileRowIter<'a> {
    // Opaque iterator state borrowing FileMetadata immutably.
}

impl FileRowIter<'_> {
    pub fn next_into(&mut self, data: &mut [u8]) -> anyhow::Result<bool>;
}

pub struct FileRows<'a> {
    // Opaque standard Iterator state borrowing FileMetadata immutably.
}

impl Iterator for FileRows<'_> {
    type Item = anyhow::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item>;
}
```

- Construction shall fail if `element_size` is zero.
- `open_or_create` shall open existing files for reading and writing without
  truncating them, or create a missing file. `FileMetadata` shall own the
  resulting `File`.
- `file_len` shall return the current physical file length.
- `element_count` shall compute the count from the current file length,
  configured offset, and element size.
- `data_offset` and `element_size` shall return the current configuration
  values without mutating state. After a successful metadata update,
  `data_offset()` shall report the new metadata length.
- All fields shall remain private. No method shall expose a reference or
  ownership of the underlying `File`.
- `get_raw_metadata` shall fill the caller's buffer with exactly the prefix
  bytes.
- `update_raw_metadata` shall replace the prefix and preserve the complete
  row-region byte sequence, shifting it when the prefix length changes. It
  shall update `data_offset` only after a successful operation.
- `truncate_rows` shall retain the first requested number of rows;
  `truncate_rows_front` shall retain the last requested number of rows and
  compact them to the start of the row region. A zero count removes all rows
  while preserving metadata.
- `get_raw_metadata`, `read_row`, and `write_row` take `&mut self` when
  required to seek the owned file handle. `get_raw_metadata` requires a buffer
  whose length equals `data_offset`; `read_row` requires a buffer whose length
  equals `element_size` and fills it with the requested row. `write_row`
  overwrites an existing row and requires exactly one row's worth of bytes.
- Row I/O is unbuffered at the wrapper/API level; each call operates on one
  row or one contiguous block and does not rely on a caller-managed buffered
  reader or writer.
- `read_rows(start, data)` and `write_rows(start, data)` shall transfer
  `data.len() / element_size` contiguous rows beginning at `start`. The byte
  slice is the concatenation of rows in ascending index order. Both methods
  require a slice length that is an exact multiple of `element_size`; unlike
  the single-row methods, they do not take a separate row count.
- Block operations shall validate row-count and byte-offset arithmetic before
  I/O. `read_rows` requires the entire range to contain existing rows.
  `write_rows` may start anywhere through the current logical end and may
  extend the logical end with a contiguous block, including a block that
  overwrites an existing prefix and appends its suffix. A start beyond the
  current end is rejected to prevent gaps. After a successful extending
  write, the current instance's logical row count shall become the end of the
  written range. A zero-length slice is a no-op if `start` is at most the
  current row count, and is out of range otherwise.
- Block reads and writes shall seek once to the first row and transfer the
  contiguous byte slice without allocating a row buffer per row. An I/O error
  shall be returned; block writes are not transactional, so an I/O failure
  may leave a prefix of the requested bytes written. The logical count is
  updated only after the complete `write_all` succeeds.
- `append_row(row)` shall require exactly `element_size` bytes and add the row
  at the byte position immediately after the last complete row. If the file
  has trailing incomplete-row bytes, the appended row shall overwrite the
  beginning of that invalid suffix and the file shall be truncated immediately
  after the appended row.
- `insert_row(index, row)` shall require exactly `element_size` bytes and
  accept indices from zero through the current complete row count, inclusive.
  Existing complete rows at or after `index` shall move one row-width later;
  the inserted bytes shall occupy the resulting gap. Incomplete trailing
  bytes shall be ignored and discarded; only complete rows are shifted and
  preserved.
- Append and insertion shall update the current instance's logical row count.
  If reserved capacity is available, they shall leave the physical file length
  unchanged; otherwise, they shall extend it only as far as required for the
  new row. Insertion at the current row count shall have the same resulting
  logical layout as append. These operations shall preserve the metadata
  prefix and shall not reinterpret row contents.
- Before a non-empty `write_row`, `write_rows`, `append_row`, or `insert_row`
  operation accesses row data, it shall extend a file shorter than
  `data_offset` with zero bytes up to that offset. Existing bytes shall not be
  overwritten. `write_row` remains replacement-only and shall reject an
  index that does not exist. `write_rows` may extend at or across the
  contiguous logical end, but shall reject a starting index beyond that end.
  An empty block write is a no-op and shall not pad.
- `reserve_rows(new_count)` shall compare the requested capacity with the
  current reserved capacity. If `new_count` is less than or equal to that
  capacity, it shall return success without modifying the file.
- If `new_count` exceeds current capacity, `reserve_rows` shall zero-pad a
  missing metadata prefix, preserve every existing byte at its current offset
  (including an incomplete trailing suffix), and set the file length to
  `data_offset + new_count * element_size`. Newly extended bytes shall read
  as zero. This changes capacity only, not the current instance's logical row
  count.
- `reserve_rows` shall check multiplication and addition for overflow before
  modifying the file. It reserves file length only; it does not promise that
  the operating system has physically allocated disk blocks. This avoids
  repeated file-length growth during later appends within the reserved range.
- Input-shape and arithmetic-overflow validation shall happen before
  modifying the file. Once a valid non-empty write operation needs to
  normalize a missing metadata prefix, that zero-padding may remain even if
  the subsequent row-range check fails.
- Insertion may move later data using bounded temporary storage, but shall
  not require loading the entire shifted region into memory. Neither
  insertion nor append promises transactional rollback if an I/O error occurs
  after file contents have begun changing.
- Read operations (`get_raw_metadata`, `read_row`, and `read_rows`) shall
  borrow `FileMetadata` immutably and use positioned reads that do not alter
  the file cursor. Unix and Windows implementations shall handle short reads
  and interruptions until the requested buffer is filled or an I/O error
  occurs.
- `iter_rows` shall snapshot the complete row count when called and iterate
  from row zero. `iter_rows_range(start, count)` shall validate the requested
  range against the current row count and iterate that range only. Both
  methods shall return an iterator that holds a shared immutable borrow of
  the `FileMetadata`, allowing other readers to operate concurrently.
- `FileRowIter::next_into` shall fill the caller's exact-`element_size`
  buffer with the next row and return `Ok(true)`. It shall return `Ok(false)`
  when the iterator is exhausted. It shall not allocate a row buffer.
- The iterator is a custom buffer-filling interface rather than a standard
  `Iterator` because each call writes into a caller-owned reusable buffer
  instead of yielding an allocated row or a borrow into internal storage.
- `rows` shall snapshot the complete row count and yield rows from zero in
  ascending order. `rows_range(start, count)` shall validate and snapshot the
  requested range at construction. The returned `FileRows` shall implement
  the standard `Iterator` trait so it can be used with `for` loops and
  iterator adaptors.
- Each successful `FileRows::next` item shall be an owned `Vec<u8>` exactly
  `element_size` bytes long. This conventional API accepts one row allocation
  per successful item; callers prioritizing allocation avoidance shall use
  `FileRowIter::next_into`.
- A row read failure shall be yielded as `Some(Err(error))`. The iterator
  shall become exhausted after the first error so it does not repeatedly
  yield errors for the same row.
- The wrapper shall not implement `Clone`. A caller needing an additional
  handle must create it explicitly before transferring the owned `File` to
  the wrapper.

## Errors and failure behavior

- Construction with a zero element size shall return an error.
- Path-based open or create failures, including inaccessible parent
  directories and permission errors, shall be returned to the caller.
- Opening an existing file shall not silently replace or truncate its data.
- Operating-system errors from retrieving file metadata shall be returned to
  the caller by `file_len` and `element_count`.
- A data offset beyond the current file length shall produce an element count
  of zero, not arithmetic underflow or a panic.
- A file shorter than one complete element in its data region shall have an
  element count of zero.
- A trailing partial element shall be ignored by the count operation and
  shall not be treated as an error.
- A metadata buffer whose length differs from `data_offset`, or a row read
  buffer whose length differs from `element_size`, shall return an error
  without reading a partial result.
- Reading metadata from a file shorter than `data_offset` shall return an
  unexpected-end-of-file error rather than returning a truncated metadata
  prefix.
- Metadata replacement, row truncation, and row I/O failures shall be
  returned to the caller. A failed operation shall not report success.
- A row index outside the existing complete-row range, or a row write with an
  incorrect byte length, shall return an error without extending the data
  region. A valid-width non-empty row write may first zero-pad a missing
  metadata prefix as specified above.
- A row read that cannot retrieve the complete requested row shall return an
  error rather than a partial successful row.
- A block buffer whose length is not an exact multiple of `element_size` or
  whose row range overflows shall return an error before file mutation. A
  non-empty block write may first zero-pad a missing metadata prefix; if its
  range then extends beyond the current complete-row count, it shall return
  an error without extending the data region.
- A zero-length block shall succeed without changing file contents when its
  start index is within or exactly at the end of the current row range; a
  start index beyond that range shall return an error.
- Block I/O failures shall be returned to the caller. A failed block read
  may have modified part of its destination buffer, and a failed block write
  may have modified part of the file; neither operation promises atomicity.
- `append_row` and `insert_row` shall reject a row buffer whose length differs
  from `element_size`. `insert_row` shall reject an index greater than the
  current logical row count after ensuring the metadata prefix exists.
- Row writes, append, and insertion shall zero-fill a missing metadata
  prefix. Replacement writes shall still reject absent rows and shall not
  extend or pad the row region. Invalid input lengths shall fail before
  metadata padding.
- Append or insertion size/offset arithmetic overflow and filesystem errors
  shall be reported. If an I/O error occurs after mutation begins, the file
  may contain a partially completed operation; success shall not be reported.
- `reserve_rows` shall return an error for size arithmetic overflow, failure
  to create a missing metadata prefix, or operating-system errors. A failed
  reservation shall not be reported as success; partial file-length changes may remain
  if the platform reports failure after making changes.
- Row count parameters greater than the current logical row count shall
  return an error for either truncation operation.
- A range whose start/count overflows or extends beyond the complete row
  count shall be rejected when creating the range iterator.
- While rows remain, `next_into` shall reject a buffer whose length differs
  from `element_size`. Once exhausted, it shall return `Ok(false)` without
  modifying the supplied buffer.
- If an external handle changes or truncates the file during iteration, a
  failed or incomplete row read shall be reported as an error, not as a
  successful row.
- A `FileRows` iterator shall return `Some(Err(_))` for a row read failure,
  then `None` on subsequent calls.
- Errors shall not be silently converted into successful zero lengths or
  counts.

## Safety and security

- Safe Rust is sufficient; `unsafe` code is not required.
- The wrapper reports filesystem metadata and does not decode or validate
  application-defined bytes in the file.
- The data offset and element size are configuration, not a protection
  boundary. This component does not lock, encrypt, authenticate, or otherwise
  protect file contents.
- The wrapper does not provide locking. Immutable positioned reads can run
  concurrently, but callers must prevent writes and structural mutations from
  overlapping other accesses, including through additional file handles.
- The wrapper does not promise atomic rollback if an operating-system error
  occurs partway through resizing metadata or moving rows. Such failures must
  be reported; callers may need to validate or recover the file.

## Compatibility

- The wrapper is a new non-generic API; it does not promise source
  compatibility with the generic legacy `FileReference<T>`.
- Length/count queries and positioned read operations shall not alter the
  file's contents or cursor position.
- File length includes bytes reserved for metadata, while element count
  excludes the configured prefix and any trailing incomplete element.
- Metadata resizing preserves row bytes but changes their physical offsets;
  front truncation compacts retained rows to the first row position.
- Direct row reads and writes are not buffered, and row payloads remain opaque
  byte sequences. Block reads and writes transfer contiguous caller-owned
  slices without allocating per-row buffers.
- Appending and inserting preserve opaque row bytes; insertion shifts
  subsequent complete rows rather than decoding or rebuilding records.
  Incomplete trailing bytes are ignored and discarded by append/insertion.
- Row reservation extends file length with `set_len`, preserves the byte
  positions of existing data and partial suffix bytes, and leaves logical row
  count unchanged for the current instance. Newly extended bytes read as zero.
- `reserve_rows` is a no-op when the requested capacity is already satisfied.
- No element encoding format or cross-platform record layout is defined here.

## Performance and resource use

- Length and count queries shall not read the file contents.
- `element_count` may perform one file metadata query and constant-time
  arithmetic.
- Metadata resizing may move the row region and may require bounded
  temporary storage; it shall not require loading the entire row region into
  memory. Metadata and row read methods shall use the caller-provided buffers
  and shall not allocate output vectors.
- Row read/write methods operate on one row per call and do not maintain a
  wrapper-owned row buffer. Block read/write methods operate on contiguous
  slices in a single logical transfer and do not allocate row buffers.
- Appending writes one row; insertion may require moving the remaining file
  region and shall use bounded temporary storage rather than buffering the
  entire region.
- `reserve_rows` extends logical file length but does not guarantee physical
  disk allocation; filesystems may represent the extension sparsely. Appends
  within the reserved length avoid extending the file. The operation does not
  promise transactional rollback if the operating system reports a failure.
- The row iterator shall reuse only the caller's buffer and shall not allocate
  per-row output storage. Its captured end bound prevents later file growth
  from extending an in-progress iteration.
- The standard `FileRows` iterator allocates a `Vec<u8>` for each successful
  row item; this is intentional to provide conventional owned iterator items.
- The wrapper owns one file handle. Dropping it closes that handle according
  to standard `File` semantics; any additional handles are caller-owned.
- No caching guarantee, synchronization guarantee, or throughput target is
  imposed.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps                                                                                                                                                                                                                                                                                                                       | Dependencies | Completion criteria                                                                                                                                                                                                                                                                                      |
| -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P-001    | Confirm public naming, owned-file behavior, path open-or-create behavior, metadata layout/resize semantics, row truncation, single-row, block, append, insert, reserve operations, both iterator behaviors, and error semantics against this requirement and legacy `FileReference`.                                                                 | None.        | The API is non-generic, takes ownership of `File`, stores the offset as `u64`, preserves existing files and row bytes on metadata resize/reservation, and defines row bounds, block sizes/ranges, append/insert layout, reservation behavior, truncation, and iterator semantics.                        |
| P-002    | Implement `FileMetadata` with owned-file and path-based constructors, metadata access/update, row truncation, direct single-row and block I/O, append/insert/reserve, buffer-filling iterators, and conventional `Iterator` implementations for full/ranged rows.                                                                                    | P-001.       | The API satisfies ownership and byte-layout requirements, creates missing files, preserves existing data, extends file length for reserved capacity without changing the current logical row count, and all row operations and iterator interfaces follow their specified allocation and error behavior. |
| P-003    | Add tests for both constructors, metadata access and resizing, row truncation from both ends, single-row and block I/O, append/insert/reserve, full/ranged buffer-filling and standard iteration, file length/count, boundaries, caller-managed handles, and operating-system errors where a deterministic test is available; run applicable checks. | P-002.       | All specified behaviors and deterministically testable failure cases are tested, and the relevant project checks pass.                                                                                                                                                                                   |

### Current implementation checklist

- [x] P-001 — `Confirm file metadata API contract` — Complete
- [x] P-002 — `Implement owned-file metadata wrapper` — Complete
- [x] P-003 — `Test and validate file metadata wrapper` — Complete

**Planning reflection (complete before implementation):**

- Separating contract confirmation, implementation, and validation lets the
  file-size/count semantics be settled before code depends on them.
- P-002 depends on the decisions in P-001; tests and project checks depend on
  the concrete API in P-002.
- The principal edge cases are a zero element size, an offset beyond EOF, and
  trailing bytes that do not complete an element. Fallible filesystem
  metadata access is required to avoid success-shaped defaults.
- Path opening must distinguish creation from truncation: create missing
  files, but preserve existing file contents.
- Metadata is the exact prefix `[0, data_offset)`. Reads fill caller-provided
  buffers, avoiding both output allocations and a wrapper-owned cache.
- The wrapper takes ownership of one `File`; it does not clone or share the
  file or metadata offset. Callers remain responsible for any additional
  handles and write coordination. Read methods and iterators use immutable
  borrows and positioned reads, so multiple readers can share the wrapper
  without racing on a mutable file cursor.
- Front truncation retains the last `row_count` rows; back truncation retains
  the first `row_count`. Both reject counts larger than the current complete
  row count. Row writes replace existing rows only; `append_row` adds one
  record and `insert_row` shifts complete later records to make room.
- Block reads and writes use a caller-provided contiguous byte slice whose
  length determines the number of rows. Buffer shape and arithmetic are
  validated before mutation; non-empty writes first zero-pad a missing
  metadata prefix and then validate the row range. Empty blocks are no-ops at
  any position from the start through the end of the row range. Block writes
  are not atomic on I/O failure.
- Append and insertion accept one exact-width row slice. They ignore and
  discard incomplete trailing bytes; insertion shifts only later complete
  rows while preserving their order. Insertion at `element_count()` appends.
- Non-empty row writes, append, and insertion zero-pad a missing metadata
  prefix to `data_offset`. Indexed writes remain replacement-only and fail if
  the target rows do not exist; prefix padding may remain after such a range
  error.
- `reserve_rows` is monotonic: requests at or below current capacity are
  strict no-ops. Growth zero-pads a missing metadata prefix, keeps all existing
  bytes at their current offsets, exposes zero-filled extension bytes, and
  leaves this instance's logical row count unchanged. Reopening derives the
  row count from file length, so reserved rows then appear as rows.
- Iteration covers either all complete rows or a validated range. Its custom
  `next_into` method uses the caller's reusable buffer to avoid allocating a
  row for each step.
- In addition, `rows` and `rows_range` return standard iterators yielding
  `Result<Vec<u8>>` for idiomatic `for` loops and iterator adaptors. The
  per-row allocation is intentional; callers can choose `next_into` to avoid
  it.

**Implementation reflection (update as work progresses):**

- Implemented `FileMetadata` in `src/file/file_metadata.rs` and exported it
  from the already-public `file` module. Metadata movement and row insertion
  use bounded 64 KiB copying in the safe overlap direction.
- Row reservation extends file length with `set_len`; this grows its logical
  length but does not guarantee physical disk allocation. Appends and
  insertions within the resulting capacity do not extend the file.
- Added 31 executable integration tests covering the specified constructors,
  metadata operations, row operations, iterators, concurrent positioned
  reads, failure handling, and reservation behavior. The metadata-query
  OS-error case remains not run because there is no deterministic portable way
  to make `metadata()` fail on an open regular file.
- `cargo test --manifest-path rust_serial_file\\Cargo.toml` passed all unit,
  integration, and doc tests; Clippy with warnings denied and `cargo fmt
--check` passed.
- Coverage has not been measured. No coverage tool was run.

## Testing and acceptance criteria

### Tests

Use **⏳ Not run** until a test has been executed, then set its status to
**✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs                                                                                                                     | Description of the test                                                                                                                                                     | Success requirements                                                                                                                                                                                                                                              | Test file                                 | Test name                                            | Status     |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- | ---------------------------------------------------- | ---------- |
| T-001   | `RSF-FILE-FR-005`, `RSF-FILE-FR-006`, `RSF-FILE-RS-001`, `RSF-FILE-RS-002`                                                          | Construct the wrapper by moving an owned `File` into the non-generic type.                                                                                                  | Construction accepts `File` by value and the wrapper can query the file afterward.                                                                                                                                                                                | `rust_serial_file/tests/file_metadata.rs` | `constructs_from_owned_file`                         | ✅ Success |
| T-002   | `RSF-FILE-FR-001`, `RSF-FILE-DS-001`                                                                                                | Query the physical length of a file containing bytes.                                                                                                                       | The reported length equals the file's byte length.                                                                                                                                                                                                                | `rust_serial_file/tests/file_metadata.rs` | `reports_physical_file_length`                       | ✅ Success |
| T-003   | `RSF-FILE-FR-002`, `RSF-FILE-FR-003`, `RSF-FILE-DS-002`                                                                             | Count fixed-size elements in a file with no leading offset and an exact multiple of the element size.                                                                       | The count equals the number of complete elements.                                                                                                                                                                                                                 | `rust_serial_file/tests/file_metadata.rs` | `counts_fixed_size_elements`                         | ✅ Success |
| T-004   | `RSF-FILE-FR-004`, `RSF-FILE-FR-003`, `RSF-FILE-DS-002`                                                                             | Count elements after an offset reserved for in-file metadata.                                                                                                               | Bytes before the offset do not contribute to the element count.                                                                                                                                                                                                   | `rust_serial_file/tests/file_metadata.rs` | `excludes_leading_metadata_from_element_count`       | ✅ Success |
| T-005   | `RSF-FILE-DS-002`, `RSF-FILE-DS-003`                                                                                                | Query the count when the file is shorter than the offset or has a trailing partial element.                                                                                 | A short data region yields zero; trailing incomplete bytes are ignored without underflow.                                                                                                                                                                         | `rust_serial_file/tests/file_metadata.rs` | `handles_offset_beyond_end_and_partial_elements`     | ✅ Success |
| T-006   | `RSF-FILE-RS-003`                                                                                                                   | Attempt handle-based and path-based construction with a zero element size. | Both constructors return an error, and path-based construction does not create the file. | `rust_serial_file/tests/file_metadata.rs` | `rejects_zero_element_size` | ✅ Success |
| T-007   | `RSF-FILE-FR-007`, `RSF-FILE-FR-008`                                                                                                | Observe a file-length change made through an additional handle created and retained by the caller.                                                                          | The wrapper itself is not cloned; a fresh query sees the updated physical length.                                                                                                                                                                                 | `rust_serial_file/tests/file_metadata.rs` | `observes_length_changes_from_caller_managed_handle` | ✅ Success |
| T-008   | `RSF-FILE-RS-004`                                                                                                                   | Query metadata for a file handle whose metadata operation fails, where the platform permits a deterministic setup. No portable setup is available for an open regular file. | If the platform provides a deterministic setup, the query returns the operating-system error instead of a zero length or count.                                                                                                                                   | —                                         | —                                                    | ⏳ Not run |
| T-009   | `RSF-FILE-FR-009`, `RSF-FILE-RS-002`                                                                                                | Open the wrapper from a path that does not yet exist.                                                                                                                       | The file is created, owned by the wrapper, and initially reports a length of zero.                                                                                                                                                                                | `rust_serial_file/tests/file_metadata.rs` | `creates_missing_file_from_path`                     | ✅ Success |
| T-010   | `RSF-FILE-FR-009`                                                                                                                   | Open the wrapper from a path containing an existing file.                                                                                                                   | Existing bytes are preserved and remain included in the reported physical length.                                                                                                                                                                                 | `rust_serial_file/tests/file_metadata.rs` | `opens_existing_file_without_truncating`             | ✅ Success |
| T-011   | `RSF-FILE-FR-010`, `RSF-FILE-RS-006`                                                                                                | Read raw metadata into a caller-provided buffer.                                                                                                                            | An exact-size buffer is filled with the prefix bytes without an output allocation; buffers of the wrong size and files shorter than the prefix return errors.                                                                                                     | `rust_serial_file/tests/file_metadata.rs` | `loads_exact_raw_metadata`                           | ✅ Success |
| T-012   | `RSF-FILE-FR-011`, `RSF-FILE-DS-005`, `RSF-FILE-DS-006`                                                                             | Replace metadata with same-size, smaller, and larger byte sequences.                                                                                                        | Metadata bytes update correctly; all following row bytes, including any trailing partial row, remain identical and are shifted as needed; length and local offset update correctly.                                                                               | `rust_serial_file/tests/file_metadata.rs` | `updates_metadata_without_losing_rows`               | ✅ Success |
| T-013   | `RSF-FILE-FR-012`, `RSF-FILE-DS-007`, `RSF-FILE-RS-009`                                                                             | Truncate rows from the back to retain a selected row count, including zero and an invalid count.                                                                            | The first requested rows remain, metadata is unchanged, partial trailing bytes are removed, zero removes all rows, and a count above the current count returns an error.                                                                                          | `rust_serial_file/tests/file_metadata.rs` | `truncates_rows_from_back`                           | ✅ Success |
| T-014   | `RSF-FILE-FR-013`, `RSF-FILE-DS-007`, `RSF-FILE-RS-009`                                                                             | Truncate rows from the front to retain a selected row count.                                                                                                                | The last requested rows remain in order beginning immediately after metadata; metadata is unchanged and invalid counts return an error.                                                                                                                           | `rust_serial_file/tests/file_metadata.rs` | `truncates_rows_from_front`                          | ✅ Success |
| T-015   | `RSF-FILE-FR-014`, `RSF-FILE-DS-008`, `RSF-FILE-FR-022`, `RSF-FILE-DS-015`, `RSF-FILE-RS-006`, `RSF-FILE-RS-007`, `RSF-FILE-RS-008` | Read rows into and write rows from caller-provided exact-size byte slices.                                                                                                  | Reads fill the caller buffer without an output allocation; writes replace existing rows without changing the data-region length; valid-width non-empty writes first zero-pad a missing metadata prefix; wrong-size buffers return errors without mutation.        | `rust_serial_file/tests/file_metadata.rs` | `reads_and_writes_rows_without_buffering`            | ✅ Success |
| T-016   | `RSF-FILE-FR-015`, `RSF-FILE-RS-011`                                                                                                | Read the configured element size and data offset, then update metadata size and read the offset again.                                                                      | Getters return the configured values, reflect a successful metadata resize, and do not permit mutation or access to the file handle.                                                                                                                              | `rust_serial_file/tests/file_metadata.rs` | `exposes_read_only_layout_metadata`                  | ✅ Success |
| T-017   | `RSF-FILE-FR-016`, `RSF-FILE-DS-009`, `RSF-FILE-RS-012`                                                                             | Iterate all rows using one reusable caller-provided buffer.                                                                                                                 | Complete rows are copied into the buffer in ascending order, iteration stops at its initial row-count bound, and no row output vector is allocated.                                                                                                               | `rust_serial_file/tests/file_metadata.rs` | `iterates_all_rows_into_reusable_buffer`             | ✅ Success |
| T-018   | `RSF-FILE-FR-016`, `RSF-FILE-DS-009`, `RSF-FILE-RS-012`                                                                             | Iterate a selected row range, including an empty range, and request invalid ranges.                                                                                         | Exactly the requested rows are copied into the buffer in order; an empty range is immediately exhausted; overflow or out-of-bounds ranges return errors at creation.                                                                                              | `rust_serial_file/tests/file_metadata.rs` | `iterates_selected_row_range`                        | ✅ Success |
| T-019   | `RSF-FILE-FR-016`, `RSF-FILE-RS-012`                                                                                                | Call `next_into` with an incorrectly sized buffer while rows remain and after exhaustion.                                                                                   | A wrong-sized active buffer returns an error without advancing; after exhaustion `next_into` returns `Ok(false)` and leaves the buffer unchanged.                                                                                                                 | `rust_serial_file/tests/file_metadata.rs` | `validates_row_iterator_buffer_size`                 | ✅ Success |
| T-020   | `RSF-FILE-FR-017`, `RSF-FILE-DS-009`, `RSF-FILE-DS-010`, `RSF-FILE-RS-013`                                                          | Use the standard full-row iterator with a `for` loop and iterator adaptors.                                                                                                 | It yields `Ok` owned byte vectors in ascending order, each exactly one row long, and stops at the row-count snapshot taken when created.                                                                                                                          | `rust_serial_file/tests/file_metadata.rs` | `standard_iterator_yields_all_rows`                  | ✅ Success |
| T-021   | `RSF-FILE-FR-017`, `RSF-FILE-DS-009`, `RSF-FILE-DS-010`, `RSF-FILE-RS-013`                                                          | Use the standard range iterator for a valid range and an empty range, and request a range outside the file.                                                                 | It yields exactly the requested owned rows in ascending order; an empty range yields no items; invalid ranges are rejected when creating the iterator.                                                                                                            | `rust_serial_file/tests/file_metadata.rs` | `standard_iterator_yields_selected_range`            | ✅ Success |
| T-022   | `RSF-FILE-FR-017`, `RSF-FILE-DS-010`                                                                                                | Cause a row read failure during standard iteration, then call `next` again.                                                                                                 | The failing row yields `Some(Err(_))`, and the iterator returns `None` thereafter.                                                                                                                                                                                | `rust_serial_file/tests/file_metadata.rs` | `standard_iterator_stops_after_read_error`           | ✅ Success |
| T-023   | `RSF-FILE-FR-018`, `RSF-FILE-DS-011`, `RSF-FILE-DS-012`, `RSF-FILE-RS-014`                                                          | Read multiple contiguous rows into a caller-provided buffer, including an empty block, and try a partial-row buffer and an out-of-range block.                              | Complete rows are copied in ascending order without output allocation; valid empty blocks succeed; invalid sizes and ranges return errors without partial-range reads.                                                                                            | `rust_serial_file/tests/file_metadata.rs` | `reads_contiguous_row_blocks`                        | ✅ Success |
| T-024   | `RSF-FILE-FR-019`, `RSF-FILE-DS-011`, `RSF-FILE-DS-012`, `RSF-FILE-RS-014`                                                          | Write contiguous rows including overwrite, extension across the logical end, empty and gapped blocks, and a partial-row buffer.                                            | Existing rows are replaced in order; a block starting at or before the logical end may append a contiguous suffix and updates the count on success; empty blocks do not pad; gaps and malformed buffers return errors.                                         | `rust_serial_file/tests/file_metadata.rs` | `writes_contiguous_row_blocks`                       | ✅ Success |
| T-025   | `RSF-FILE-FR-020`, `RSF-FILE-FR-022`, `RSF-FILE-DS-013`, `RSF-FILE-DS-015`, `RSF-FILE-RS-015`                                       | Append one row to empty and non-empty data regions, including a file with trailing incomplete-row bytes and one shorter than `data_offset`; reject an incorrect row size.   | The row overwrites the incomplete suffix, the file ends immediately after the new complete row, missing metadata is zero-padded, and invalid input length causes no mutation.                                                                                     | `rust_serial_file/tests/file_metadata.rs` | `appends_rows_and_discards_partial_suffix`           | ✅ Success |
| T-026   | `RSF-FILE-FR-021`, `RSF-FILE-FR-022`, `RSF-FILE-DS-014`, `RSF-FILE-DS-015`, `RSF-FILE-RS-015`                                       | Insert rows at the beginning, middle, and end with a trailing incomplete suffix or a file shorter than `data_offset`; reject an invalid index and row size.                 | Inserted rows occupy the requested index, later complete rows remain byte-for-byte ordered, incomplete trailing bytes are discarded, and the missing prefix is zero-padded.                                                                                       | `rust_serial_file/tests/file_metadata.rs` | `inserts_rows_and_discards_partial_suffix`           | ✅ Success |
| T-027   | `RSF-FILE-FR-014`, `RSF-FILE-FR-019`, `RSF-FILE-FR-022`, `RSF-FILE-DS-012`, `RSF-FILE-DS-015`, `RSF-FILE-RS-016`                   | Attempt a non-empty single-row replacement and a gapped block write on a file shorter than `data_offset`, then append a block at the logical end.                         | The invalid writes are rejected after prefix zero-padding; the valid block appends a contiguous row, preserves prefix bytes, and updates logical count.                                                                                                          | `rust_serial_file/tests/file_metadata.rs` | `pads_metadata_before_nonempty_writes`                | ✅ Success |
| T-028   | `RSF-FILE-FR-023`, `RSF-FILE-DS-016`                                                                                                | Reserve zero, the current capacity, and a larger capacity, including a file with a short metadata prefix.                                                                   | Requests at or below capacity do not change file length; growth zero-pads missing metadata and preserves existing bytes; newly extended bytes are zero, current logical row count is unchanged, and appending/inserting within capacity does not extend the file. | `rust_serial_file/tests/file_metadata.rs` | `reserves_rows_and_preserves_existing_offsets`       | ✅ Success |
| T-029   | `RSF-FILE-FR-023`                                                                                                                   | Request a row count whose target file size overflows and attempt reservation through a read-only file handle.                                                               | Overflow and file-extension failures are returned; no success-shaped result is reported.                                                                                                                                                                          | `rust_serial_file/tests/file_metadata.rs` | `reports_row_reservation_failures`                   | ✅ Success |
| T-030   | `RSF-FILE-FR-023`, `RSF-FILE-DS-016`                                                                                                | Reopen a file after reserving capacity without appending all reserved rows.                                                                                                 | The original instance retains its logical count; a newly opened instance interprets complete rows implied by file length as existing rows.                                                                                                                        | `rust_serial_file/tests/file_metadata.rs` | `reopening_treats_reserved_rows_as_existing_rows`    | ✅ Success |
| T-031   | `RSF-FILE-FR-019`, `RSF-FILE-DS-002`, `RSF-FILE-DS-012`, `RSF-FILE-DS-016`                                                          | Extend logical rows by writing a contiguous block into previously reserved capacity.                                                                                      | The block is written without changing reserved physical length, and the current instance's logical row count increases to include the new rows.                                                                                                                | `rust_serial_file/tests/file_metadata.rs` | `writes_rows_into_reserved_capacity_and_updates_logical_count` | ✅ Success |
| T-032   | `RSF-FILE-RS-018`                                                                                                                   | Run concurrent reads of metadata and indexed/block rows through shared references to one `FileMetadata`.                                                                    | Every reader receives the requested bytes without interference from another reader's file position or buffer.                                                                                                                                               | `rust_serial_file/tests/file_metadata.rs` | `supports_concurrent_positioned_reads`                 | ✅ Success |

### Acceptance criteria

1. The wrapper reports physical file length and complete fixed-size element
   count using a configured prefix offset.
2. The public wrapper is non-generic, owns its `File`, and stores the offset
   as a plain `u64`. All fields remain private; immutable getters expose only
   the element size and offset, never the file handle.
3. Path-based construction creates missing files and preserves existing
   contents.
4. Metadata can be read into a caller-provided buffer and updated at equal,
   smaller, and larger sizes without losing row bytes.
5. Back and front truncation preserve the metadata and the specified rows.
6. Direct single-row and block reads/writes, append/insert, and full/ranged
   buffer-filling iteration use caller-provided buffers; append/insertion
   discard invalid trailing partial rows and row mutations pad missing
   metadata bytes. Row reservation obeys its monotonic no-op rule, preserves
   existing byte offsets, and extends the file to the requested capacity
   without changing the current instance's logical row count.
   Conventional full/ranged iterators implement `Iterator` and yield fallible
   owned row vectors.
7. Row reservation returns observable file-extension errors and does not
   promise physical disk-block allocation.
8. Zero element size, offset boundaries, trailing partial elements, and
   filesystem errors have explicit tested behavior.
9. Length/count queries and positioned reads do not change file contents or
   cursor position.
10. Applicable formatting, lint, build, and test checks pass.

## Code coverage

Record coverage for the wrapper's production module after implementation.
Include the coverage tool, measured scope, percentages, and any remaining
uncovered behavior. Do not exclude relevant production code to increase the
reported percentage.

- **Coverage tool:** `cargo-llvm-cov` 0.9.1, via
  `cargo llvm-cov --manifest-path rust_serial_file\\Cargo.toml --summary-only`.
- **Measured scope:** The complete `src/file/file_metadata.rs` production
  module, exercised by the full crate test suite (139 tests passed).
- **Coverage summary:** 94.76% lines (362/382), 84.89% regions (629/741),
  and 92.50% functions (37/40).
- **Uncovered behavior:** Twenty source lines and three functions remain
  uncovered, primarily defensive error-handling paths such as allocation and
  file-operation failures. T-008 remains **⏳ Not run** because this platform
  has no deterministic portable way to make `File::metadata()` fail for a
  valid open regular file; the implementation propagates its error directly.

## Out of scope

- Typed serialization or deserialization of row contents; row I/O only
  transfers opaque fixed-size byte sequences.
- Parsing, validating, or interpreting the application-level format of the
  metadata or row bytes.
- Protecting or synchronizing access to file contents.
- Internal cloning, reference-counting, or synchronization of the file handle.
- Generic abstractions parameterized by an element struct or record type.
- Handling multiple disjoint data regions or metadata sections within a file.
- Guaranteeing a consistent snapshot while another handle modifies the file.
