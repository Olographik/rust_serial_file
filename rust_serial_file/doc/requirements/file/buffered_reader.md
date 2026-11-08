# Buffered Reader Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Define a buffered, read-only row reader that wraps `FileMetadata` and provides indexed and iterator access to raw byte rows.

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

Specify a buffered, read-only view over a file-backed collection of fixed-size
rows. `BufferedReader` borrows a `FileMetadata`, using its row layout
and block-read operations to load consecutive rows into an in-memory cache.
Requests that fall within the cached range are served from memory; requests
outside it refill the cache from the file.

Rows are opaque byte sequences rather than values of a generic record type.
Direct row access returns a slice borrowed from the cache. The reader also
provides a standard iterator yielding owned `Vec<u8>` rows, a custom iterator
that fills a caller-provided row buffer, and a lending iterator that returns
slices into the cache.

The reader snapshots the logical row count at construction. The caller
retains ownership of `FileMetadata` and is responsible for coordinating any
other access to the underlying file, including access through other handles
or processes.

## Goals and scope

### Goals

- Borrow a caller-owned `FileMetadata` instance without exposing its file
  handle.
- Provide direct indexed reads of opaque fixed-size byte rows.
- Reuse a contiguous row cache for random and sequential row access.
- Refill the cache using `FileMetadata` block reads rather than one file read
  per requested row.
- Provide caller-configurable cache capacity and a sensible default.
- Provide indexed reads and three iterator styles with explicit bounds and
  observable storage errors.
- Provide a caller-buffer row read path to avoid allocating a row `Vec`.

### Scope

- Specify the buffered reader type, cache semantics, constructors, and
  read-only methods.
- Specify buffer refill, cache hits, row-index validation, and iterator
  behavior.
- Specify direct row access, full/ranged owned-row iteration, and
  full/ranged caller-buffer iteration.
- Reuse `FileMetadata` for row layout, row count, and contiguous file reads.
- Leave file creation, metadata interpretation, and row mutation to
  `FileMetadata` or other components.

## Assumptions and dependencies

- `FileMetadata` owns the underlying `std::fs::File` and provides
  `element_size()`, `element_count()`, and `read_rows(start, buffer)`.
- The row width comes from `FileMetadata`; the reader does not interpret row
  contents or require a Rust record type.
- The default cache target is 64 KiB. It is converted to a whole-row
  capacity, with capacity clamped to at least one row even when one row
  exceeds 64 KiB.
- An explicit cache capacity is expressed as a positive number of rows.
- The reader snapshots `FileMetadata::element_count()` when constructed.
  Changes made through another file handle or process while the reader is
  alive are unsupported; the caller is responsible for coordinating them.
  Cached data and snapshot length are not refreshed.
- Existing project Rust edition, MSRV, dependencies, and lint policies apply.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| `RSF-BR-FR-001` | The component shall provide a public `BufferedReader` that borrows a caller-owned `FileMetadata` and reads its fixed-size rows without modifying the file. |
| `RSF-BR-FR-002` | The reader shall provide indexed row reads that return a slice borrowed from its cache without interpreting row contents or allocating an output vector. |
| `RSF-BR-FR-003` | The reader shall provide a caller-buffer method to read a row into a mutable slice without allocating an output row. |
| `RSF-BR-FR-004` | The reader shall load a contiguous range of rows into its cache on a cache miss and serve subsequent requests for rows in that range from the cache. |
| `RSF-BR-FR-005` | The reader shall support a default cache capacity based on a 64 KiB target and a constructor that accepts an explicit positive row capacity. |
| `RSF-BR-FR-006` | The reader shall provide standard full and ranged iterators that yield owned byte rows in ascending index order. |
| `RSF-BR-FR-007` | The reader shall report out-of-range row access as an error and shall not perform a cache refill for that request. |
| `RSF-BR-FR-008` | The reader shall keep its `FileMetadata`, cache storage, and cache bounds private and shall not expose mutable access to the wrapped metadata. |
| `RSF-BR-FR-009` | The reader shall obtain row layout and file reads through `FileMetadata`; it shall not access or clone the underlying file handle directly. |
| `RSF-BR-FR-010` | The reader shall provide custom full and ranged iterators whose `next_into` method fills a caller-provided row buffer. |
| `RSF-BR-FR-011` | The file module shall expose the owned-row, caller-buffer, and cache-slice iterator types returned by `BufferedReader`. |
| `RSF-BR-FR-012` | The reader shall provide full and ranged lending iterators whose `next` method returns a borrowed slice of the next row in the internal cache. |
| `RSF-BR-FR-013` | The reader shall provide `reset_capacity(row_capacity)` to change its cache capacity at runtime; a changed capacity shall resize the existing cache and invalidate its contents, while setting the current capacity shall be a no-op. |

### Data and serialization

| ID | Requirement |
|---|---|
| `RSF-BR-DS-001` | A row is exactly `FileMetadata::element_size()` opaque bytes; rows are indexed from zero and retain their file order. |
| `RSF-BR-DS-002` | The reader shall capture the logical row count during construction. Its `len()` and all row bounds shall use that snapshot for the lifetime of the reader. |
| `RSF-BR-DS-003` | The cache shall contain a contiguous row interval described by its first row index and row count. A refill for row `i` shall begin at `(i / capacity) * capacity`. Cache capacity and byte-length arithmetic shall be checked for overflow. |
| `RSF-BR-DS-004` | A refill shall load no more than the configured capacity and shall stop at the captured logical row count. The final cache interval may be shorter than its capacity. |
| `RSF-BR-DS-005` | A failed cache refill shall be returned to the caller and shall not publish a partially filled cache as valid data. |
| `RSF-BR-DS-006` | External file changes after construction are outside the consistency contract. The component shall not detect or reconcile them; the caller is responsible for coordinating access through other handles or processes. |
| `RSF-BR-DS-007` | `reset_capacity` shall change only cache capacity and contents; it shall not change the captured row count or access the file. When the capacity changes, the next row read shall refill using the new capacity; setting the current capacity shall leave the existing cache valid. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| `RSF-BR-RS-001` | `BufferedReader` shall be a concrete, non-generic type. |
| `RSF-BR-RS-002` | The reader shall borrow `FileMetadata` mutably for its lifetime, shall not own or implement `Clone` for it, and shall not internally synchronize access. |
| `RSF-BR-RS-003` | Construction and cache allocation shall be fallible and report errors through `anyhow::Result`; allocation failure shall not panic or be converted to success. |
| `RSF-BR-RS-004` | The reader shall expose its captured row count as `u64` without performing fallible file I/O. |
| `RSF-BR-RS-005` | Iterator items shall be `anyhow::Result<Vec<u8>>`; the iterator shall visit its captured range in ascending order and report per-row read errors as `Err` items. |
| `RSF-BR-RS-006` | The caller-buffer read method shall require a buffer exactly equal to the configured row width and shall not allocate an output vector. |
| `RSF-BR-RS-007` | An explicit cache capacity of zero rows shall be rejected before allocating the cache. |
| `RSF-BR-RS-008` | The custom iterator's `next_into` method shall fill an exact-width caller buffer and return `anyhow::Result<bool>`, where `false` indicates exhaustion. |
| `RSF-BR-RS-009` | The lending iterator shall expose `next(&mut self) -> Option<anyhow::Result<&[u8]>>`; a successful row slice borrows the reader's cache and remains valid only until the iterator is mutably accessed again. |
| `RSF-BR-RS-010` | `reset_capacity` shall reject zero capacity and return an error if resizing cache storage fails; on failure, the prior capacity and cache shall remain unchanged. |

## Public API

The required public type is `BufferedReader`, exported from
`rust_serial_file::file`.

The following signatures are illustrative; names may follow existing module
conventions as long as the required behavior is preserved:

```rust
pub struct BufferedReader<'a> { /* private fields */ }
pub struct BufferedRows<'a> { /* private fields */ }
pub struct BufferedRowIter<'a> { /* private fields */ }
pub struct BufferedRowSliceIter<'iter, 'metadata> { /* private fields */ }

impl<'a> BufferedReader<'a> {
    pub fn new(metadata: &'a mut FileMetadata) -> anyhow::Result<Self>;

    pub fn with_capacity(
        metadata: &'a mut FileMetadata,
        row_capacity: usize,
    ) -> anyhow::Result<Self>;

    pub fn reset_capacity(&mut self, row_capacity: usize) -> anyhow::Result<()>;

    pub fn len(&self) -> u64;

    pub fn read_row(
        &mut self,
        index: u64,
    ) -> anyhow::Result<&[u8]>;

    pub fn read_row_into(
        &mut self,
        index: u64,
        data: &mut [u8],
    ) -> anyhow::Result<()>;

    pub fn iter_rows(&mut self) -> BufferedRowIter<'_>;

    pub fn iter_rows_range(
        &mut self,
        start: u64,
        count: u64,
    ) -> anyhow::Result<BufferedRowIter<'_>>;

    pub fn rows(&mut self) -> BufferedRows<'_>;

    pub fn rows_range(
        &mut self,
        start: u64,
        count: u64,
    ) -> anyhow::Result<BufferedRows<'_>>;

    pub fn iter_row_slices(&mut self) -> BufferedRowSliceIter<'_, 'a>;

    pub fn iter_row_slices_range(
        &mut self,
        start: u64,
        count: u64,
    ) -> anyhow::Result<BufferedRowSliceIter<'_, 'a>>;
}

impl BufferedRowIter<'_> {
    pub fn next_into(&mut self, data: &mut [u8]) -> anyhow::Result<bool>;
}

impl<'iter, 'metadata> BufferedRowSliceIter<'iter, 'metadata> {
    pub fn next(&mut self) -> Option<anyhow::Result<&[u8]>>;
}

impl Iterator for BufferedRows<'_> {
    type Item = anyhow::Result<Vec<u8>>;
}
```

- `new` shall use the default cache capacity: at least one row, targeting
  64 KiB of row data.
- `with_capacity` shall interpret `row_capacity` as rows, not bytes.
- `reset_capacity` shall accept a positive capacity in rows. If the requested
  capacity differs, it shall resize the existing cache, invalidate cached
  rows, and use the new capacity on the next read. If the requested capacity
  is unchanged, it shall be a no-op and retain the current cache.
- `reset_capacity` shall not alter the row-count snapshot or underlying file.
  If the new capacity is zero or cache resizing fails, it shall return an
  error and leave the prior capacity and cache usable.
- `read_row_into` shall reuse the cache and copy the requested row to the
  exact-width caller buffer.
- `len` shall return the row-count snapshot taken during construction.
- `read_row` shall return a slice of exactly one row borrowed from the
  internal cache. The returned slice is valid only until the reader is
  mutably accessed again.
- `iter_rows` and `iter_rows_range` shall return custom iterators that fill a
  caller-provided reusable buffer. The ranged constructor shall validate the
  range against the captured row count.
- `rows` and `rows_range` shall return standard iterators with owned row
  values. The ranged constructor shall validate the range against the
  captured row count, and row read failures shall be yielded as `Err` items.
- `iter_row_slices` and `iter_row_slices_range` shall return lending
  iterators. Each successful `next` returns a slice of exactly one row from
  the internal cache; the caller must stop using the slice before calling
  `next` again. The ranged constructor shall validate its range against the
  captured row count.
- A lending iterator shall use `None` for exhaustion, `Some(Ok(row))` for a
  successful row, and `Some(Err(error))` for a failed cache refill. After a
  refill error it shall be exhausted.
- All iterator styles shall visit their captured ranges in ascending index
  order and stop after the final row. An empty range yields no rows.
- `BufferedReader` shall not expose file mutation methods or access to the
  borrowed `FileMetadata`.

## Errors and failure behavior

- Construction shall return an error if querying the initial row count fails,
  if cache sizing arithmetic overflows, or if allocating cache storage fails.
- Construction with zero explicit row capacity shall return an error.
- `reset_capacity(0)` shall return an error without changing the existing
  capacity or cache. Cache-size arithmetic and row-capacity conversion shall
  be validated before resizing; row refills shall use the validated capacity.
- A cache allocation or resize error during `reset_capacity` shall be
  observable and leave the prior capacity and cache usable.
- Row reads shall return an error for an index greater than or equal to the
  captured length.
- `read_row_into` shall return an error if the supplied buffer does not
  exactly match the configured row width.
- Errors from `FileMetadata::read_rows` shall be propagated. Failed refills
  shall not be reported as successful reads or expose partially loaded bytes.
- `read_row` shall not allocate an output row. Cache refill allocation and
  file-read failures shall be reported.
- Custom iterator reads shall return errors directly from `next_into`;
  standard and lending iterator reads shall yield errors as `Err` items and
  then become exhausted.
- Ranged iterator construction shall return an error for an overflowing range
  or a range extending beyond the captured row count.
- The reader shall not silently return zero-filled or stale data after a
  failed file read.
- A custom iterator shall return `Ok(false)` after exhaustion without reading
  from the file or requiring a row buffer.
- A lending iterator shall return `None` after exhaustion. Its returned row
  slice shall not outlive the mutable borrow of the iterator, preventing a
  cache refill while a caller still uses that slice.
- This component performs no file writes, so it has no persistence or
  rollback behavior.

## Safety and security

- No `unsafe` code is required or permitted unless separately justified and
  reviewed.
- File bytes are opaque input. The reader shall validate indices, buffer
  widths, and allocation arithmetic before accessing cached data.
- The reader does not deserialize rows into types, validate application-level
  contents, or treat file bytes as trusted.
- Cached bytes remain in memory until replaced or the reader is dropped.
  Secure zeroization is not required by this component.
- The reader does not synchronize access to the borrowed `FileMetadata` or
  underlying file. The caller is responsible for coordinating other handles
  and processes and preventing conflicting access while the reader is in use.

## Compatibility

- The implementation shall use the existing `FileMetadata` API for layout and
  block reads and shall not change `FileMetadata` behavior.
- The reader API shall remain a file-specific row reader and shall not depend
  on collection search or mutation traits.
- The byte layout and row order remain exactly those defined by
  `FileMetadata`; no on-disk format or metadata changes are introduced.
- Cache capacity affects performance and memory use only, not the values or
  ordering returned by the reader.
- No migration of existing files is required.

## Performance and resource use

- The reader shall keep at most one cache window, containing no more than the
  configured number of rows.
- A cache miss shall use one contiguous `FileMetadata::read_rows` operation
  for the selected window; a cache hit shall not perform file I/O.
- The default cache byte target is 64 KiB. When the row width exceeds this
  target, the cache contains one row. Explicit capacity may use more or less
  memory than the default.
- `read_row` and the lending iterator return borrowed cache slices without
  allocating output rows. Each successful standard iterator item allocates
  an owned row. The custom iterator fills the caller's reusable buffer
  without allocating an output row.
- `read_row_into` and custom iterator reads copy from the internal cache into
  the caller's buffer. Lending iterator reads do not copy row data out of the
  cache.
- `reset_capacity` resizes the existing cache storage. A changed capacity
  invalidates cached contents and the next read incurs a cache refill; an
  unchanged capacity does not resize or invalidate the cache.
- All iterator styles shall reuse the cache across adjacent rows and shall
  not cause a separate file read for every row when multiple rows fit in a
  refill.
- The reader provides no synchronization against other file handles or
  processes. The caller is responsible for coordinating concurrent access.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Confirm the `BufferedReader` API, cache sizing and refill rules, row-count snapshot, and error behavior. | None. | This requirement defines all public behavior, boundaries, dependencies, and observable failures needed for implementation. |
| P-002 | Implement `BufferedReader`, cache management and reset, caller-buffer reads, direct row access, and all iterator styles. | P-001. | The implementation borrows `FileMetadata`, uses contiguous block reads on cache misses, invalidates the cache on successful capacity reset, and propagates errors. |
| P-003 | Add unit and integration tests for cache behavior, capacities, row boundaries, iteration, and failures; run project validation. | P-002. | All deterministic requirements have tests, relevant test suites pass, and formatting and lint checks pass. |

### Current implementation checklist

- [x] P-001 — `Specify buffered reader contract` — Complete
- [x] P-002 — `Implement buffered reader` — Complete
- [x] P-003 — `Test and validate buffered reader` — Complete

**Planning reflection (complete before implementation):**

- P-001 settles the behavioral contract before cache implementation.
- P-002 uses `FileMetadata::read_rows` for cache misses and keeps cache
  validity separate from allocated storage so failed refills cannot expose
  partially loaded data.
- P-003 exercises cache behavior through observable reads and values, rather
  than coupling tests to private field layout. The integration tests and full
  crate test suite pass; formatting and lint validation also pass.
- The main risks are handling row widths that exceed the default cache target
  and ensuring failed refills never expose partially loaded data.

**Implementation reflection (update as work progresses):**

- The lending iterator borrows the reader cache for each returned row, so it
  cannot implement the standard `Iterator` trait; its required `next` method
  is provided directly. The existing `FileMetadata` block-read API supports
  the specified borrowing and error behavior without changes.

## Testing and acceptance criteria

### Tests

Use **⏳ Not run** until a test has been executed, then set its status to
**✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `RSF-BR-FR-001`, `RSF-BR-RS-001`, `RSF-BR-RS-002` | Construct a reader from a mutable borrow of `FileMetadata`. | The caller retains the metadata owner, and the reader can use it for the duration of its borrow. | `rust_serial_file/tests/buffered_reader.rs` | `constructs_from_borrowed_file_metadata` | ✅ Success |
| T-002 | `RSF-BR-FR-005`, `RSF-BR-RS-007` | Construct with zero explicit row capacity. | Construction returns an error without allocating or panicking. | `rust_serial_file/tests/buffered_reader.rs` | `rejects_zero_cache_capacity` | ✅ Success |
| T-003 | `RSF-BR-FR-002`, `RSF-BR-DS-001`, `RSF-BR-RS-004` | Query the reader length and read a raw byte row by index. | The length is the captured complete-row count and the returned borrowed slice exactly matches the selected row without an output allocation. | `rust_serial_file/tests/buffered_reader.rs` | `reads_raw_rows_and_reports_length` | ✅ Success |
| T-004 | `RSF-BR-FR-003`, `RSF-BR-RS-006` | Read a row into a caller-provided exact-width buffer and try incorrect widths. | An exact-width buffer receives the row without an output allocation; incorrect widths return errors. | `rust_serial_file/tests/buffered_reader.rs` | `reads_into_caller_buffer` | ✅ Success |
| T-005 | `RSF-BR-FR-004`, `RSF-BR-DS-003`, `RSF-BR-DS-004` | Access rows within a cache window and then access a row outside it. | Cache hits return correct rows; a miss refills the aligned contiguous window, clipped at the snapshot length. | `rust_serial_file/tests/buffered_reader.rs` | `refills_cache_for_misses_and_reuses_hits` | ✅ Success |
| T-006 | `RSF-BR-FR-005` | Construct with the default cache using narrow rows and rows larger than the default byte target. | The default capacity targets 64 KiB, contains at least one row, and handles a row wider than the target with a one-row cache. | `rust_serial_file/tests/buffered_reader.rs` | `uses_default_capacity_for_small_and_large_rows` | ✅ Success |
| T-007 | `RSF-BR-FR-005`, `RSF-BR-DS-003` | Construct with a valid explicit cache capacity. | Capacity is interpreted in rows and the configured capacity is used for refills. | `rust_serial_file/tests/buffered_reader.rs` | `uses_explicit_cache_capacity` | ✅ Success |
| T-008 | `RSF-BR-FR-007` | Read the row at `len()` and an index beyond it. | Both accesses return errors without changing logical length or exposing cache data as a valid row. | `rust_serial_file/tests/buffered_reader.rs` | `rejects_out_of_range_rows` | ✅ Success |
| T-009 | `RSF-BR-FR-006`, `RSF-BR-RS-005` | Iterate all rows using the owned-row `rows()` iterator. | The iterator yields every row in order as `Ok(Vec<u8>)` and then terminates. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_all_rows_in_order` | ✅ Success |
| T-010 | `RSF-BR-FR-006`, `RSF-BR-FR-010`, `RSF-BR-FR-012`, `RSF-BR-RS-005`, `RSF-BR-RS-008`, `RSF-BR-RS-009` | Iterate an empty reader using all three iterator styles. | `rows()` yields no items, `iter_rows().next_into(...)` returns `Ok(false)`, and `iter_row_slices().next()` returns `None`. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_empty_reader` | ✅ Success |
| T-011 | `RSF-BR-FR-004`, `RSF-BR-FR-006`, `RSF-BR-FR-010`, `RSF-BR-FR-012`, `RSF-BR-DS-004` | Iterate across multiple cache windows and a final partial window using all three iterator styles. | Each iterator yields every captured row exactly once and cache refills do not skip or duplicate rows. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_across_cache_windows` | ✅ Success |
| T-012 | `RSF-BR-DS-002`, `RSF-BR-DS-006` | Change file length through another handle after reader construction. | `len()` remains the captured count; the caller is responsible for preventing conflicting external access. | `rust_serial_file/tests/buffered_reader.rs` | `keeps_construction_row_count_snapshot` | ✅ Success |
| T-013 | `RSF-BR-DS-005` | Cause deterministic cache refill failures and exercise owned and lending iterator read errors. | Failed reads return errors, partial cache data is not exposed, and each iterator becomes exhausted after an error. | `rust_serial_file/tests/buffered_reader.rs` | `propagates_cache_refill_errors_without_reusing_partial_data` | ✅ Success |
| T-014 | `RSF-BR-RS-003`, `RSF-BR-DS-003` | Exercise cache sizing and owned-row allocation failures. | Invalid arithmetic and unallocatable requests return errors without panic or success-shaped results. | `rust_serial_file/tests/buffered_reader.rs`; `rust_serial_file/src/file/buffered_reader.rs` | `reports_cache_size_failures`; `reports_owned_row_allocation_overflow` | ✅ Success |
| T-015 | `RSF-BR-FR-010`, `RSF-BR-RS-008` | Read all rows and a selected range into a reusable caller buffer. | The custom iterator fills each exact-width buffer in order, returns false at exhaustion, and rejects invalid range or buffer sizes. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_rows_into_reusable_buffer` | ✅ Success |
| T-016 | `RSF-BR-FR-006` | Iterate a selected row range with the owned-row iterator. | `rows_range` yields exactly the requested rows in order and rejects invalid or overflowing ranges. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_selected_owned_row_range` | ✅ Success |
| T-017 | `RSF-BR-FR-012`, `RSF-BR-RS-009` | Iterate full and ranged rows through the lending iterator while reading each returned slice before advancing. | Each slice points to exactly one correct cached row, can be used without an owned row allocation, and is no longer used before the next mutable iterator call. | `rust_serial_file/tests/buffered_reader.rs` | `iterates_rows_as_cache_slices` | ✅ Success |
| T-018 | `RSF-BR-FR-013`, `RSF-BR-DS-007`, `RSF-BR-RS-010` | Increase and decrease cache capacity after loading rows, reset to the current capacity, then attempt invalid and unallocatable resets. | Changed capacities invalidate the cache and subsequent reads refill correctly; resetting to the current capacity is a no-op. Row count and file bytes remain unchanged. Invalid or unallocatable resets return errors and preserve the existing capacity/cache. | `rust_serial_file/tests/buffered_reader.rs` | `resets_capacity_and_invalidates_cache` | ✅ Success |

### Acceptance criteria

1. `BufferedReader` borrows caller-owned `FileMetadata` and is exported
   through the file module.
2. The reader returns opaque raw rows in file order through indexed reads
   borrowed from its cache, an owned-row `Iterator`, a custom caller-buffer
   iterator, and a lending iterator over cache slices.
3. Cache hits avoid file I/O; cache misses load contiguous row blocks through
   `FileMetadata`.
4. The default cache target is 64 KiB with a minimum capacity of one row, and
   callers may configure a positive row capacity.
5. The reader captures its logical row count at construction; the caller
   coordinates other access to the underlying file for the reader's lifetime.
6. Indexed reads and the lending iterator return borrowed cache slices;
   caller-buffer reads and custom iteration avoid output-vector allocation,
   while the standard iterator yields fallible owned vectors. All public row
   access validates exact width and bounds.
7. `reset_capacity` changes capacity without changing file contents or row
   count and invalidates cached data on success.
8. Storage, arithmetic, and allocation failures remain observable.
9. Deterministic boundary and failure behavior is tested; applicable format,
   lint, build, and test checks pass.

## Code coverage

Coverage was measured with `cargo-llvm-cov 0.9.1` using
`cargo llvm-cov --lib --test buffered_reader --summary-only`. The run executed
36 library unit tests and 18 buffered-reader integration tests; no production
files were excluded.

| Scope | Lines | Functions | Regions | Branches |
|---|---:|---:|---:|---:|
| `src/file/buffered_reader.rs` | 100% (236/236) | 100% (27/27) | 96.36% (318/330) | Not instrumented (0 reported) |

No executable source lines or functions remain uncovered. The 12 missed
regions are error-context and checked-conversion/arithmetic expression paths;
they do not leave executable lines uncovered. In particular, row-width
conversion failures cannot be produced through `FileMetadata`, which rejects
zero-width rows and stores the width as `u64`.

## Out of scope

- Decoding raw rows into generic Rust types or `SerializedStruct` values.
- Writing, appending, inserting, or truncating file rows.
- Changing `FileMetadata` behavior or defining another on-disk format.
- Persisting cache state or reserved capacity across reopen.
- Providing a coherent snapshot in the presence of external file mutation.
- Thread-safe shared access, synchronization, or locking.
- Multi-window caching, prefetch threads, asynchronous I/O, and configurable
  eviction policies.
