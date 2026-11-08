# Buffered Writer Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Define a buffered, write-only row writer that borrows `FileMetadata` and batches fixed-size row writes.

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

Specify a buffered, write-only wrapper over a caller-owned `FileMetadata`.
`BufferedWriter` stages fixed-size opaque rows in a bounded in-memory cache
and writes them through `FileMetadata` when the cache is full, an operation
moves outside the active cache window, the caller explicitly flushes, or the
writer is dropped.

The writer accepts individual rows and contiguous blocks of rows. A block
larger than the cache capacity first flushes pending cached rows, then writes
the supplied block directly through `FileMetadata::write_rows` without
splitting it into cache-sized pieces.

The caller retains ownership of `FileMetadata` and coordinates any other
access to the underlying file. The writer does not implement or adapt the
`SerialDataWriter` trait.

`flush` means writing pending cache contents through `FileMetadata`; it does
not promise durable storage equivalent to `File::sync_data`. A flush called
explicitly reports errors. `Drop` attempts to flush, but Rust does not allow
`Drop` to return an error, so callers requiring observable persistence must
call `flush` before dropping the writer.

## Goals and scope

### Goals

- Borrow a caller-owned `FileMetadata` instance mutably without exposing its
  underlying file handle.
- Buffer single-row writes and blocks of rows using a bounded cache.
- Flush pending rows on explicit request, cache exhaustion, write-window
  changes, and drop.
- Preserve file layout and row semantics by using `FileMetadata` write APIs.
- Write blocks larger than the cache directly after first flushing pending
  cached data.
- Report write, validation, and allocation failures through `anyhow::Result`
  on fallible operations.

### Scope

- Specify `BufferedWriter`, its cache configuration, row and block writes,
  flush behavior, and drop behavior.
- Specify cache-window transitions, capacity boundaries, error behavior, and
  interaction with `FileMetadata`.
- Preserve the fixed-width opaque row layout and existing file contents
  outside explicitly written rows while allowing appends at the logical end.
- Leave file creation, metadata interpretation, and row insertion to
  `FileMetadata` or other components.

## Assumptions and dependencies

- `FileMetadata` owns the underlying `std::fs::File` and provides
  `element_size()`, `element_count()`, `write_row(index, row)`, and
  `write_rows(start, data)`, which may append a contiguous suffix.
- `FileMetadata::write_row` overwrites an existing logical row.
  `FileMetadata::write_rows` overwrites existing rows and may also extend
  the logical row count when the block reaches or crosses the current logical
  end without a gap.
- A row is an opaque byte sequence of exactly `FileMetadata::element_size()`
  bytes. A block is a contiguous byte slice containing an integral number of
  rows.
- The writer captures the initial logical row count and row width at
  construction. Its logical row count grows immediately when contiguous
  appends are accepted, including appends still pending in the cache. A write
  may overwrite an existing row or append starting exactly at the current
  logical end; gaps beyond the end are rejected. External file changes while
  the writer is alive are unsupported; the caller coordinates other handles
  or processes.
- Cache capacity is expressed as a positive row count. The default cache
  targets 64 KiB of row data, rounds down to a whole-row count, and holds at
  least one row.
- A write window begins at the index of the first write staged in that
  window and spans up to the configured capacity. It may include slots at
  the current logical end for contiguous pending appends. Writes whose start
  or complete range does not fit the active window cause pending dirty rows
  to flush before the new window is used.
- Within an active window, arbitrary rows may be staged or overwritten. The
  writer tracks modified rows so flushing never writes over rows that were
  not explicitly staged.
- The active window is considered full when every row in that window has a
  pending write; it is flushed immediately.
- Existing project Rust edition, MSRV, dependencies, and lint policies apply.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| `RSF-BW-FR-001` | The component shall provide a public `BufferedWriter` that mutably borrows a caller-owned `FileMetadata` and writes fixed-size rows without exposing the underlying file handle. |
| `RSF-BW-FR-002` | The writer shall provide a method to stage one exact-width row at an existing row index or append one row when the index equals the current logical row count. |
| `RSF-BW-FR-003` | The writer shall provide a method to stage a contiguous block of complete rows that overwrites existing rows, appends at the logical end, or overwrites an existing prefix and appends its contiguous suffix. |
| `RSF-BW-FR-004` | The writer shall buffer writes in a bounded cache and flush pending writes when an operation starts outside the active cache window. |
| `RSF-BW-FR-005` | The writer shall flush pending rows when the active window becomes full, when the caller explicitly invokes `flush`, and when the writer is dropped. |
| `RSF-BW-FR-006` | A block containing more rows than the cache capacity shall first flush pending cached rows, then bypass the cache and pass the entire contiguous block to `FileMetadata::write_rows` without cache-sized chunking; `write_rows` shall support contiguous extension beyond the logical end. |
| `RSF-BW-FR-007` | A block that fits the cache capacity shall be staged in the active window when it fits; otherwise pending writes shall be flushed before staging the block in a new window. |
| `RSF-BW-FR-008` | A successful explicit `flush` shall write all pending dirty rows and leave the cache empty of pending modifications. |
| `RSF-BW-FR-009` | The writer shall use `FileMetadata::write_row` or `write_rows` for all file writes and shall not clone or otherwise access the underlying file handle. |
| `RSF-BW-FR-010` | The writer shall append rows contiguously at the logical end and update its logical row count; it shall not insert rows or implement the `SerialDataWriter` trait. |
| `RSF-BW-FR-011` | The file module shall expose `BufferedWriter` and its public constructors and methods. |

### Data and serialization

| ID | Requirement |
|---|---|
| `RSF-BW-DS-001` | Rows are exactly `FileMetadata::element_size()` opaque bytes; row indices begin at zero and preserve the existing file order. |
| `RSF-BW-DS-002` | The writer shall capture the row width and initial logical row count at construction. Its logical row count shall grow as appends are accepted, including pending appends, and all subsequent write bounds shall use the updated count. |
| `RSF-BW-DS-003` | A block buffer shall contain a whole number of rows. Its row count shall be derived from its byte length and the captured row width using checked conversions. |
| `RSF-BW-DS-004` | The cache shall hold no more than its configured capacity of rows and shall track which rows contain pending writes. Flushing shall write only pending rows, not untouched rows within the active window. |
| `RSF-BW-DS-005` | A block larger than the configured capacity shall not be copied into the cache; after pending writes flush successfully, the entire block shall be passed directly to `FileMetadata::write_rows`, which shall overwrite its existing-row prefix and append any contiguous suffix. |
| `RSF-BW-DS-006` | The writer shall not promise a transactional block write. If a file write fails after partially modifying the file, the error shall be reported and rollback is not required. |
| `RSF-BW-DS-007` | The writer shall not detect or reconcile external changes to the file while alive. The caller is responsible for coordinating access through other handles or processes. |
| `RSF-BW-DS-008` | A flush shall write pending bytes through `FileMetadata`; it shall not imply an operating-system durability guarantee such as `sync_data`. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| `RSF-BW-RS-001` | `BufferedWriter` shall be a concrete, non-generic type that mutably borrows `FileMetadata` for its lifetime and does not own or clone it. |
| `RSF-BW-RS-002` | Constructors, write methods, and explicit `flush` shall report failures through `anyhow::Result`. |
| `RSF-BW-RS-003` | Construction and cache allocation shall be fallible; zero capacity, arithmetic overflow, and allocation failure shall return errors without panic or success-shaped fallback. |
| `RSF-BW-RS-004` | `write_row(index, row)` shall require a row slice exactly equal to the captured row width. |
| `RSF-BW-RS-005` | `write_rows(start, data)` shall reject a byte length that is not an exact multiple of the captured row width. |
| `RSF-BW-RS-006` | A write may end at or before the current logical row count, or extend it only when its range starts at or before the current end with no gap. A start index beyond the current end shall return an error. Empty block writes shall validate their start index and otherwise perform no write. |
| `RSF-BW-RS-007` | `Drop` shall attempt to flush pending rows and shall not panic. Since `Drop` cannot return errors, callers requiring error visibility shall explicitly call `flush` before drop. |
| `RSF-BW-RS-008` | The writer shall not implement `SerialDataWriter` or expose generic collection operations. |

## Public API

The required public type is `BufferedWriter`, exported from
`rust_serial_file::file`.

The following signatures are illustrative; names may follow existing module
conventions as long as the required behavior is preserved:

```rust
pub struct BufferedWriter<'a> { /* private fields */ }

impl<'a> BufferedWriter<'a> {
    pub fn new(metadata: &'a mut FileMetadata) -> anyhow::Result<Self>;

    pub fn with_capacity(
        metadata: &'a mut FileMetadata,
        row_capacity: usize,
    ) -> anyhow::Result<Self>;

    pub fn write_row(&mut self, index: u64, row: &[u8]) -> anyhow::Result<()>;

    pub fn write_rows(&mut self, start: u64, rows: &[u8]) -> anyhow::Result<()>;

    pub fn flush(&mut self) -> anyhow::Result<()>;

    pub fn len(&self) -> u64;
}
```

- `new` shall use the default cache capacity based on a 64 KiB byte target,
  with at least one row.
- `with_capacity` shall interpret capacity as a positive row count.
- `len` shall return the writer's current logical row count without file I/O.
  It begins at the count captured during construction and increases as
  contiguous append rows are accepted into the writer, before they are
  flushed to disk.
- `write_row` shall validate the exact row width and require `index <= len()`.
  An index less than `len()` overwrites that existing or pending row; an index
  equal to `len()` appends one row and increases `len()` immediately. A
  greater index is rejected.
- `write_rows` shall validate the complete byte slice and range before
  changing pending cache state. Its start may be at most `len()`; a range
  extending past `len()` appends the contiguous suffix and increases `len()`
  when accepted. If the block exceeds the configured cache capacity, it shall
  flush pending rows and write the existing-row prefix and appended suffix
  directly through `FileMetadata::write_rows`, without copying the block into
  the cache.
- A buffered block that overwrites existing rows and appends a suffix shall
  validate the complete request before modifying pending data; an invalid
  range or gap shall reject the whole request.
- For writes within cache capacity, a starting index outside the active
  window shall flush the pending dirty rows before establishing a new window.
  A write entirely contained in the active window may stage or replace rows
  there. A block that does not fit the active window shall flush pending rows
  and start a new window at its requested start index.
- Filling all rows of the active window with pending writes shall flush it
  immediately and clear its pending state.
- For cached append rows, a successful flush shall append each pending row
  exactly once and keep the writer's logical count consistent with accepted
  pending appends. If a flush partially appends then fails, retry behavior
  shall not duplicate rows already appended successfully.
- `flush` shall be idempotent when there are no pending writes.
- `Drop` shall attempt the equivalent of `flush`; a failure cannot be
  returned from `Drop`. Callers needing a reliable error result shall call
  `flush` explicitly.

## Errors and failure behavior

- Construction shall return an error if querying the row count fails, if row
  width conversion or cache sizing overflows, or if cache or dirty-state
  allocation fails.
- Construction with zero explicit row capacity shall return an error.
- Row writes shall return errors for an index greater than the current logical
  row count or for a row slice with an incorrect width.
- Block writes shall return errors for a non-integral row byte length, range
  overflow, a start index beyond the logical end, or a range that would leave
  a gap before its first appended row.
- Errors from `FileMetadata::write_row` and `write_rows` shall be propagated
  by explicit writes and `flush`.
- If flushing pending data before a window change or oversized block fails,
  the triggering operation shall return an error and shall not proceed to
  stage or directly write its new data.
- A failed flush shall retain uncommitted pending data for a later explicit
  retry. Successfully completed append rows shall not be appended again on
  retry. The underlying file may nevertheless have been partially modified;
  no rollback is promised.
- If an oversized direct block write fails, the error shall be returned.
  Already-flushed pending rows remain written. The writer's logical length
  shall reflect only successful `write_rows` calls; no transaction is
  implied.
- `Drop` shall make a best-effort flush and shall not panic. Errors during
  drop cannot be returned to the caller; callers that need to observe them
  must call `flush` before the writer is dropped.
- An empty block shall not allocate or access the file. Its start index shall
  still be validated against the current logical row count, allowing the
  one-past-end index for a zero-length range.
- No write failure shall be converted into a successful return.

## Safety and security

- No `unsafe` code is required or permitted unless separately justified and
  reviewed.
- File bytes are opaque. The writer shall validate row widths, block
  alignment, range arithmetic, and cache sizing before copying or writing.
- The writer does not serialize values or validate application-level content.
- Cached bytes remain in memory until flushed or the writer is dropped.
  Secure zeroization is not required by this component.
- The writer does not synchronize access to `FileMetadata` or the underlying
  file through other handles or processes. The caller is responsible for
  preventing conflicting access while the writer is in use.
- Drop-time flushing is best-effort and must not panic during unwinding.

## Compatibility

- The implementation shall use the existing `FileMetadata` API for layout,
  row count, and file writes; it shall not change `FileMetadata` behavior.
- The writer shall remain a file-specific component and shall not implement,
  depend on, or adapt `SerialDataWriter`.
- Existing file format, metadata prefix, row width, row order, and rows not
  explicitly written shall remain unchanged. Successful appends extend the
  file by whole rows at its logical end.
- Buffer capacity affects memory use and write timing only, not the resulting
  bytes.
- No migration of existing files is required.

## Performance and resource use

- The writer shall keep one cache window of at most the configured row
  capacity and track pending rows with bounded metadata.
- A single-row write shall not require a separate file write until a flush
  condition occurs.
- Contiguous pending overwrites shall be flushed using as few `write_rows`
  operations as practical; untouched rows shall not be written merely because
  they fall inside the active window. Contiguous pending appends shall be
  combined into `write_rows` calls beginning at the current logical end.
- A cache-full event, explicit flush, window transition, and drop each flush
  pending data before the relevant operation completes.
- A block larger than the cache capacity shall bypass cache copying. After
  pending cache data has flushed, the entire block is passed to one
  `FileMetadata::write_rows` call, which writes existing rows and extends the
  logical end for any contiguous suffix.
- The default cache target is 64 KiB of row data. A row wider than the target
  uses a one-row cache. Explicit capacity may use more or less memory.
- `flush` is a logical write-through to `FileMetadata`, not a promise that
  file contents have been forced to durable media.
- The writer provides no synchronization against other file handles or
  processes. The caller is responsible for coordinating concurrent access.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Confirm ownership, row/block semantics, append behavior, cache-window behavior, flush triggers, oversized-block handling, and error behavior. | None. | This requirement defines all public behavior, boundaries, dependencies, and observable failures needed for implementation. |
| P-002 | Implement `BufferedWriter`, bounded dirty-row cache, row overwrites and appends, block writes, flush transitions, and best-effort drop flushing. | P-001. | The writer borrows `FileMetadata`, preserves untouched rows, appends only at the contiguous logical end, uses direct writes for oversized blocks, and propagates explicit-operation errors. |
| P-003 | Add unit and integration tests for cache transitions, flush behavior, append and block boundaries, errors, and drop behavior; run formatting, lint, build, and test validation. | P-002. | All deterministic requirements have tests and all applicable project checks pass. |
| P-004 | Run coverage for the complete `BufferedWriter` production module and add tests for uncovered reachable behavior until line coverage reaches 100%. | P-003. | The unfiltered production module reports 100% line coverage. If coverage is below 100%, this phase remains incomplete and the writer is not accepted until additional tests reach the threshold. |

### Current implementation checklist

- [x] P-001 — `Specify buffered writer contract` — Complete
- [ ] P-002 — `Implement buffered writer` — Not started
- [ ] P-003 — `Test and validate buffered writer` — Not started
- [ ] P-004 — `Measure and complete production coverage` — Not started

**Planning reflection (complete before implementation):**

- Separating contract definition from cache implementation and validation
  establishes the write/flush semantics before handling dirty rows.
- P-002 depends on P-001; P-003 depends on the implementation existing; P-004
  depends on the production module and its behavioral tests being complete.
- Key risks are not overwriting untouched rows, preserving pending writes
  after a flush error, and accounting for errors during `Drop`, which cannot
  be returned.
- Oversized blocks deliberately bypass the cache after pending writes flush;
  they are not divided into cache-sized chunks.
- P-004 is an acceptance gate, not a reporting-only step: if complete-module
  line coverage is below 100%, the phase stays incomplete while reachable
  uncovered behavior is investigated and tested.

**Implementation reflection (update as work progresses):**

- No implementation has started. Revisit the cache-window and dirty-row
  design if the existing `FileMetadata` API cannot preserve untouched rows
  while batching buffered writes.

## Testing and acceptance criteria

### Tests

Use **⏳ Not run** until a test has been executed, then set its status to
**✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `RSF-BW-FR-001`, `RSF-BW-RS-001` | Construct a writer from a mutable borrow of `FileMetadata`. | The caller retains the metadata owner, and the writer can use it for the duration of its borrow. | `rust_serial_file/tests/buffered_writer.rs` | `constructs_from_borrowed_file_metadata` | ⏳ Not run |
| T-002 | `RSF-BW-FR-004`, `RSF-BW-RS-003` | Construct with zero capacity and an unrepresentable or unallocatable cache size. | Construction returns an error without panic or file modification. | `rust_serial_file/tests/buffered_writer.rs` | `rejects_invalid_cache_capacity` | ⏳ Not run |
| T-003 | `RSF-BW-FR-002`, `RSF-BW-DS-001`, `RSF-BW-RS-004`, `RSF-BW-RS-006` | Stage a valid single-row overwrite, append at `len()`, and reject incorrect widths and gaps. | Valid bytes overwrite or append exactly one row; length increases on accepted append; invalid inputs return errors without corrupting pending data. | `rust_serial_file/tests/buffered_writer.rs` | `buffers_single_row_writes_and_appends` | ⏳ Not run |
| T-004 | `RSF-BW-FR-003`, `RSF-BW-DS-003`, `RSF-BW-RS-005`, `RSF-BW-RS-006` | Stage a block that overwrites existing rows and appends a contiguous suffix; reject misaligned, gapped, or overflowing blocks. | Complete rows are written in order; accepted appended rows increase length immediately; malformed and invalid ranges return errors. | `rust_serial_file/tests/buffered_writer.rs` | `buffers_multiple_rows_and_appends` | ⏳ Not run |
| T-005 | `RSF-BW-FR-004`, `RSF-BW-DS-004` | Write rows within one active window, including repeated writes to the same row. | Pending values are updated correctly, and rows not written by the caller remain unchanged. | `rust_serial_file/tests/buffered_writer.rs` | `preserves_untouched_rows_in_cache_window` | ⏳ Not run |
| T-006 | `RSF-BW-FR-004`, `RSF-BW-FR-005` | Write outside the active window after staging rows. | The pending window is flushed before the new window is staged; both sets of rows have the expected values. | `rust_serial_file/tests/buffered_writer.rs` | `flushes_on_cache_window_change` | ⏳ Not run |
| T-007 | `RSF-BW-FR-005`, `RSF-BW-FR-008` | Fill the active cache window with pending row writes. | The full window is flushed automatically and no dirty data remains pending. | `rust_serial_file/tests/buffered_writer.rs` | `flushes_when_cache_is_full` | ⏳ Not run |
| T-008 | `RSF-BW-FR-005`, `RSF-BW-FR-008` | Explicitly flush pending writes, flush again, and verify file contents. | The first flush writes all pending rows, the second is a no-op, and explicit flush errors are returned. | `rust_serial_file/tests/buffered_writer.rs` | `flushes_explicitly_and_is_idempotent` | ⏳ Not run |
| T-009 | `RSF-BW-FR-005`, `RSF-BW-RS-007` | Drop a writer with pending data. | Drop attempts to flush pending data and does not panic. | `rust_serial_file/tests/buffered_writer.rs` | `flushes_pending_rows_on_drop` | ⏳ Not run |
| T-010 | `RSF-BW-FR-006`, `RSF-BW-DS-005` | Write an oversized block that overwrites existing rows and appends a suffix while other rows are pending. | Pending cache data is flushed first; the full block is sent to one `FileMetadata::write_rows` call and written in order without cache-sized chunking. | `rust_serial_file/tests/buffered_writer.rs` | `writes_oversized_block_directly_after_flush` | ⏳ Not run |
| T-011 | `RSF-BW-FR-007` | Write a block within capacity that does not fit the remaining active window. | Pending rows flush first, then the block is staged in a new window without splitting. | `rust_serial_file/tests/buffered_writer.rs` | `moves_fitting_block_to_new_window` | ⏳ Not run |
| T-012 | `RSF-BW-RS-006` | Write an empty block at a valid index, at the one-past-end index, and beyond it. | Valid empty ranges perform no I/O; indices within the empty-range boundary are accepted and out-of-range indices are rejected. | `rust_serial_file/tests/buffered_writer.rs` | `validates_empty_block_ranges` | ⏳ Not run |
| T-013 | `RSF-BW-FR-008`, `RSF-BW-DS-006` | Cause an explicit flush failure and retry after the underlying condition is corrected. | The initial error is returned, pending data remains available for retry, and successful retry writes the expected rows. | `rust_serial_file/tests/buffered_writer.rs` | `retains_pending_rows_after_flush_error` | ⏳ Not run |
| T-014 | `RSF-BW-RS-002`, `RSF-BW-FR-009` | Cause a direct oversized-block overwrite or append failure. | The error is returned; completed file operations are not rolled back or reported as pending again, and the writer length reflects completed appends. | `rust_serial_file/tests/buffered_writer.rs` | `propagates_oversized_block_write_error` | ⏳ Not run |
| T-015 | `RSF-BW-FR-010`, `RSF-BW-RS-008` | Confirm that the writer appends only at the logical end, does not insert, and does not expose generic collection operations. | Contiguous appends grow logical length; gaps are rejected; module public API has no insert method or `SerialDataWriter` implementation. | `rust_serial_file/tests/buffered_writer.rs` | `appends_without_inserting_rows` | ⏳ Not run |
| T-016 | `RSF-BW-DS-002`, `RSF-BW-DS-007` | Stage a write, truncate the file through another handle, then flush. | The writer retains its logical count and propagates the resulting I/O error; it does not reconcile the external change. | `rust_serial_file/tests/buffered_writer.rs` | `keeps_writer_row_count_and_rejects_external_changes` | ⏳ Not run |

### Acceptance criteria

1. `BufferedWriter` borrows caller-owned `FileMetadata` and is exported
   through the file module.
2. Single-row and contiguous block writes validate width and bounds and
   preserve untouched file rows.
3. Cache-window changes, cache-full events, explicit flushes, and drop flush
   pending rows according to the specified contract.
4. Oversized blocks flush pending cache contents and bypass cache copying;
   the complete block is passed to `FileMetadata::write_rows` without
   cache-sized chunking, and that API supports contiguous extension.
5. Explicit operation errors remain observable; drop-time flush is
   best-effort and does not panic.
6. Writes may append only at the current logical end; accepted buffered
   appends immediately increase `len()`, while gaps and insertion are
   rejected. The writer does not implement `SerialDataWriter`.
7. Deterministic boundary and failure behavior is tested; applicable format,
   lint, build, and test checks pass.
8. The complete `BufferedWriter` production module has 100% line coverage.
   Coverage below 100% leaves P-004 incomplete and blocks acceptance until
   tests cover the remaining reachable production lines.

## Code coverage

Record coverage for the complete `BufferedWriter` production module after
implementation. Include the coverage tool, measured scope, percentages, and
remaining uncovered behavior. Do not exclude relevant production code to
increase the reported percentage.

- Use **⏳ Not run** until coverage has been measured.
- **Coverage summary:** ⏳ Not run. Implementation has not started.

## Out of scope

- Implementing `SerialDataWriter` or generic record serialization.
- Inserting rows or creating gaps before appended rows.
- Changing `FileMetadata` behavior or defining another on-disk format.
- Durable-media synchronization guarantees such as `File::sync_data`.
- Transactional rollback for partial operating-system writes.
- Returning a drop-time flush error; callers must flush explicitly to observe
  persistence errors.
- Thread-safe shared access, synchronization, locking, or coordination with
  other file handles and processes.
- Multiple cache windows, asynchronous I/O, background flushing, and
  configurable eviction policies.
