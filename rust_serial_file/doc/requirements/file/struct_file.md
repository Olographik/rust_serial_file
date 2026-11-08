# StructFile Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Specify a typed, file-backed serial collection that owns `FileMetadata` and implements `SerialDataContainer`.

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

Define `StructFile<T>`, a typed serial collection backed by a `FileMetadata`
that it owns. The type uses `T: SerializedStruct` to encode values into and
decode values from fixed-width rows. It implements the existing
`SerialDataContainer<T>` contract so collection clients can read, iterate,
append, insert, replace, remove, clear, truncate, update, and search file-backed
records using the same API as other serial collections.

The file metadata prefix remains opaque to `StructFile`; the component only
interprets the row region. The caller chooses and manages metadata bytes when
constructing the `FileMetadata`.

## Goals and scope

### Goals

- Own a `FileMetadata` and bind its fixed row width to `T::SIZE`.
- Provide typed fallible row reads and writes using `SerializedStruct`.
- Implement `SerialDataReader<T>`, `SerialDataWriter<T>`, and therefore
  `SerialDataContainer<T>`.
- Preserve serial collection semantics, including ordered insertion,
  replacement, removal, and truncation.
- Keep file I/O, serialization, and index-conversion failures observable.

### Scope

- Specify the `StructFile<T>` type, constructors, access to the owned
  `FileMetadata`, and typed row operations.
- Specify the collection trait implementation and its iterator.
- Specify the behavior of collection mutations that shift file-backed rows.
- Preserve `FileMetadata` layout and behavior and the existing serial
  collection traits.

## Assumptions and dependencies

- The component uses the existing `FileMetadata` type and its row methods:
  `element_size`, `element_count`, `read_row`, `write_row`, `write_rows`,
  `append_row`, `insert_row`, and `truncate_rows`.
- `T` implements `SerializedStruct`; `T::SIZE` is the fixed serialized row
  width. Implementations of `SerializedStruct` are responsible for providing
  compatible `EncodeRaw` and `DecodeRaw` behavior.
- `FileMetadata` remains the sole owner of the file handle. `StructFile`
  owns the `FileMetadata` value and does not clone it.
- The row count is captured at construction and maintained by successful
  `StructFile` operations. External mutation through another file handle
  while `StructFile` is alive is unsupported; the caller must coordinate
  access.
- The component implements the existing
  `SerialDataReader<T>`, `SerialDataWriter<T>`, and
  `SerialDataContainer<T>` traits from the adapter module. Collection
  searches and `serial_auto_insert` require records to be sorted according
  to the applicable comparison, as defined by those traits.
- `write_multiple` receives records in the order the caller wants stored.
  The caller is responsible for ordering them; `StructFile` neither sorts nor
  validates their ordering.
- Existing project Rust edition, MSRV, dependencies, and lint policies apply.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| `RSF-SF-FR-001` | The component shall provide a public `StructFile<T>` that owns one `FileMetadata` and requires `T: SerializedStruct`. |
| `RSF-SF-FR-002` | Construction from `FileMetadata` shall validate that `T::SIZE` is nonzero and equals the metadata row width. |
| `RSF-SF-FR-003` | The component shall provide a path-based constructor that opens or creates a file through `FileMetadata`, using `T::SIZE` as its row width and accepting a metadata-prefix offset. |
| `RSF-SF-FR-004` | The component shall provide typed read and replacement operations for existing rows, encoding and decoding each row through `SerializedStruct`. |
| `RSF-SF-FR-005` | The component shall implement `SerialDataReader<T>`, returning owned decoded values and an iterator of fallible owned values. |
| `RSF-SF-FR-006` | The component shall implement `SerialDataWriter<T>` with append, insert, replacement, removal, removal-and-return, clear, and truncate operations using serial collection index semantics. |
| `RSF-SF-FR-007` | The component shall implement `SerialDataContainer<T>` by implementing its required reader and writer supertraits. |
| `RSF-SF-FR-008` | Insertion at an index less than the current length shall shift later records right without changing their order; insertion at the length shall append; a greater index shall fail. |
| `RSF-SF-FR-009` | Removal shall preserve the order of all remaining records, reduce the logical length by one on success, and return the removed decoded value for `serial_remove_get`. |
| `RSF-SF-FR-010` | Clear shall remove all logical rows while preserving the metadata prefix. Truncation shall retain the first requested number of rows and shall succeed without mutation when the requested length is at least the current length. |
| `RSF-SF-FR-011` | The component shall expose immutable access to the owned `FileMetadata` and allow the caller to recover ownership of it. It shall not expose mutable access that can invalidate its cached collection length. |
| `RSF-SF-FR-012` | `serial_len` and `serial_is_empty` shall be available without fallible file I/O and reflect the count maintained by the `StructFile` instance. |
| `RSF-SF-FR-013` | The component shall provide `write_multiple(&[T])` to write the supplied records in slice order starting at row zero, replacing existing rows and appending any suffix that exceeds the current row count; it shall not sort or validate input ordering. |

### Data and serialization

| ID | Requirement |
|---|---|
| `RSF-SF-DS-001` | Each logical record shall occupy exactly `T::SIZE` bytes in row order after the `FileMetadata` metadata prefix. |
| `RSF-SF-DS-002` | Construction shall initialize the logical row count from `FileMetadata::element_count`; it shall reject a count that cannot be represented as `usize`. |
| `RSF-SF-DS-003` | Encoding shall use a row buffer of exactly `T::SIZE` bytes. A successful `try_encode` must report that exactly `T::SIZE` bytes were encoded before the row can be written. |
| `RSF-SF-DS-004` | Decoding shall use exactly one complete row and `T::try_decode`; incomplete rows shall not be returned as records. |
| `RSF-SF-DS-005` | The metadata prefix shall be preserved by all collection row mutations and shall not be interpreted or rewritten by `StructFile`. |
| `RSF-SF-DS-006` | Successful append, bulk write, insert, remove, clear, and truncate operations shall update the instance's logical length to match the resulting collection. Failed operations shall not report a successful length change. |
| `RSF-SF-DS-007` | Record order shall be the physical row order. The component shall not automatically sort records except when a caller explicitly uses a sorted insertion operation from `SerialDataContainer`. |
| `RSF-SF-DS-008` | Search and sorted-insertion behavior shall follow `SerialDataContainer` comparison semantics and shall propagate undefined ordering and read errors. |
| `RSF-SF-DS-009` | `write_multiple` shall preserve the order of its input slice without sorting or validating its ordering. The caller is responsible for providing records in the desired order. It shall replace rows `[0, min(slice.len(), current_len))`, append any remaining records contiguously, and leave existing rows after the slice unchanged. An empty slice shall perform no writes. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| `RSF-SF-RS-001` | `StructFile<T>` shall be a concrete generic type with the bound `T: SerializedStruct`; it shall own `FileMetadata` and shall not implement `Clone`. |
| `RSF-SF-RS-002` | The reader iterator shall implement `Iterator<Item = anyhow::Result<T>>` and borrow the file-backed collection for its lifetime. |
| `RSF-SF-RS-003` | Collection methods shall use the existing `SerialDataReader<T>`, `SerialDataWriter<T>`, and `SerialDataContainer<T>` trait signatures and `anyhow::Result` error convention. |
| `RSF-SF-RS-004` | Public index arguments use `usize`; conversions to `FileMetadata` row indices shall be checked and report overflow. |
| `RSF-SF-RS-005` | File length shall be stored as instance-local collection state so `serial_len` does not need to perform fallible I/O. |
| `RSF-SF-RS-006` | Row and shift buffers shall use fallible allocation where allocation is required; allocation failure shall be reported rather than converted to a panic or success. |

## Public API

The type shall be available from `rust_serial_file::file` and shall implement
the adapter traits from `rust_serial_file::adapter`.

Representative API:

```rust
pub struct StructFile<T: SerializedStruct> { /* private fields */ }
pub struct StructFileIter<'a, T: SerializedStruct> { /* private fields */ }

impl<T: SerializedStruct> StructFile<T> {
    pub fn new(metadata: FileMetadata) -> anyhow::Result<Self>;

    pub fn open_or_create(
        path: impl AsRef<std::path::Path>,
        data_offset: u64,
    ) -> anyhow::Result<Self>;

    pub fn file_metadata(&self) -> &FileMetadata;

    pub fn into_file_metadata(self) -> FileMetadata;

    pub fn read_value_at(&mut self, index: usize) -> anyhow::Result<T>;

    pub fn write_value_at(&mut self, index: usize, value: &T) -> anyhow::Result<()>;

    pub fn write_multiple(&mut self, values: &[T]) -> anyhow::Result<()>;
}
```

`StructFileIter` is the associated owned-row iterator type.
`SerialDataReader<T>` shall provide the trait's required `serial_len`,
`serial_is_empty`, `serial_get`, and `serial_iter` behavior. Iteration yields
one decoded value per complete row in ascending file order; read or decode
failures are yielded as `Err` items and terminate iteration.

`SerialDataWriter<T>` shall implement:

- `serial_write(value)`: encode and append one record.
- `serial_insert(index, value)`: insert before the existing row at `index`;
  `index == serial_len()` appends.
- `serial_write_at(index, value)`: replace one existing row.
- `serial_remove(index)`: remove one existing row.
- `serial_remove_get(index)`: remove and return one existing decoded row.
- `serial_clear()`: remove all rows but retain the metadata prefix.
- `serial_truncate(len)`: retain at most the first `len` rows.

The implementation may provide additional typed convenience methods, but
they shall not conflict with or weaken the trait behavior above. No mutable
`FileMetadata` accessor shall be provided because external mutations would
make the infallible trait length stale.

`write_multiple` is a bulk replacement beginning at row zero, not an insert
or unconditional append operation. It shall preserve slice order as supplied
by the caller and shall not sort or check record ordering. When the slice is
longer than the current collection, its existing-row prefix is replaced and
the excess suffix is appended contiguously; when shorter, rows after the
written prefix remain unchanged. An empty slice performs no writes.

## Errors and failure behavior

- Constructors shall return an error for zero `T::SIZE`, a row-width
  mismatch, failure to obtain the initial row count, or a row count that
  cannot fit in `usize`.
- Read operations shall report out-of-range indices, file I/O failures,
  allocation failures, and decoding failures.
- Write operations shall report out-of-range indices, checked-conversion
  failures, allocation failures, encoding failures, and file I/O failures.
- `write_multiple` shall check row-count and byte-length arithmetic and shall
  report encoding, allocation, range-overflow, and file I/O failures. It shall
  pass records to serialization in slice order without sorting or reordering.
- If `try_encode` reports a byte count other than `T::SIZE`, the operation
  shall fail without writing that row.
- Iterator creation may return an error if its required setup fails.
  Per-record I/O or decode failures shall be yielded as `Err` items; the
  iterator shall stop after the first error rather than repeatedly reporting
  the same failing row.
- `serial_remove_get` shall decode the target row before shifting or
  truncating file contents. A decode error shall leave the collection
  unchanged.
- Index conversion and row-range arithmetic shall be checked before
  mutation. Invalid indices shall not modify collection contents.
- File writes and multi-row shifts are not transactional. If an operating
  system or serialization failure occurs after file mutation has begun, the
  error shall be returned; rollback is not required. The in-memory logical
  length shall only advance or decrease after the corresponding operation
  reports success.
- `serial_len` cannot return an error under the existing trait contract. It
  shall return the instance's last successfully maintained count; external
  mutations while the wrapper is alive are unsupported.
- No file, serialization, or decoding error shall be silently converted into
  a successful result.

## Safety and security

- No `unsafe` code is required or permitted unless separately justified and
  reviewed.
- File contents are untrusted bytes. The implementation shall validate row
  size, index bounds, range arithmetic, serialization output length, and
  allocation sizes before using row data.
- The component shall rely on `SerializedStruct::try_decode` for typed
  decoding and shall not treat file bytes as trusted application values.
- The component shall not expose the underlying file handle or a mutable
  `FileMetadata` reference that could bypass collection length maintenance.
- Serialized values and temporary row buffers remain in process memory until
  released; secure zeroization is not required.
- `StructFile` does not synchronize access through other handles or
  processes. The caller is responsible for exclusive/coordinated access.

## Compatibility

- `FileMetadata` remains the owner and authority for file layout, metadata
  prefix, and row I/O. This component shall not alter its behavior.
- The existing `SerializedStruct` wire representation and
  `SerialDataContainer` API shall be used without modification.
- The file format consists of the existing opaque metadata prefix followed
  by fixed-width rows of `T::SIZE` bytes. No type tags, schema versions, or
  additional headers are added.
- `StructFile<T>` shall preserve row order and bytes for records not affected
  by a successful collection mutation.
- Existing files opened with the matching row width and data offset require
  no migration. A row-width mismatch shall be rejected rather than
  reinterpreted.

## Performance and resource use

- Typed reads and single-record writes shall use a fixed stack buffer for rows
  up to 1 KiB and a fallibly allocated row buffer for larger rows. Bulk writes
  may allocate one buffer sized to the encoded record block.
- Iteration shall use one `BufferedReader` for its lifetime and decode
  borrowed rows from its cache without allocating a row buffer per record.
  The cache shall load file rows in cache-sized windows rather than once per
  record, without loading the complete file into memory.
- Insertion and removal shall use bounded temporary storage when shifting
  rows and shall not require allocation proportional to the entire remaining
  file.
- `serial_len` and `serial_is_empty` shall not perform file I/O.
- The collection is single-owner and provides no internal locking or
  concurrent mutation support.
- No throughput guarantee is made for insertions or removals; their
  required row shifting is proportional to the number of affected rows.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Define the typed file layout, constructors, serialization checks, trait semantics, iterator behavior, and error policy. | None. | This document defines the observable API, ownership, collection behavior, and failure cases. |
| P-002 | Implement `StructFile<T>`, construction and layout validation, typed row and bulk writes, and the fallible typed iterator. | P-001. | The type owns `FileMetadata`, rejects invalid row widths, and reads/writes exact `T::SIZE` rows with observable serialization and I/O errors. |
| P-003 | Implement `SerialDataReader<T>`, `SerialDataWriter<T>`, and `SerialDataContainer<T>` for `StructFile<T>`, including ordered shifts, removal, clear, and truncate. | P-002. | All trait methods preserve serial collection semantics and keep the maintained length consistent after successful operations. |
| P-004 | Add unit and integration tests for serialization, collection operations, boundaries, failures, iteration, and file compatibility; run format, lint, build, and test validation. | P-003. | All deterministic requirements have tests and all applicable project checks pass. |
| P-005 | Measure production-module coverage and add tests for reachable uncovered behavior. | P-004. | Coverage is reported for the complete production module, with remaining uncovered code documented and justified. |

### Current implementation checklist

- [x] P-001 — `Specify StructFile contract` — Complete
- [x] P-002 — `Implement typed file operations` — Complete
- [x] P-003 — `Implement serial container traits` — Complete
- [x] P-004 — `Test and validate StructFile` — Complete
- [x] P-005 — `Measure StructFile coverage` — Complete

**Planning reflection (complete before implementation):**

- Separating typed row conversion from collection semantics makes the
  serialization and fixed-width invariants testable before implementing
  mutations that shift multiple rows.
- P-003 depends on typed reads and writes; P-004 depends on the public
  behavior existing. Coverage measurement follows functional validation so
  tests are not written solely to satisfy a percentage.
- Key risks include stale infallible length reporting, ensuring record
  shifting preserves bytes and order, and partial file modification on I/O
  failure. External mutation is explicitly unsupported.
- The current `FileMetadata` API supports insertion and truncation but not
  arbitrary deletion. Removal therefore requires bounded forward shifting
  followed by truncation; failure after shifting starts cannot promise
  rollback.

**Implementation reflection (update as work progresses):**

- `StructFile<T>` owns `FileMetadata`, validates the nonzero row width at
  construction, and caches the row count to satisfy the infallible reader
  length API.
- Typed reads and writes use fallible serialization and checked allocation.
  Bulk writes encode the complete slice before writing, preserving input order
  and leaving existing suffix rows untouched.
- Removal shifts rows forward through a fixed-size buffer and then truncates.
  As specified, a failed I/O operation during a multi-row mutation may leave
  partial file changes; the cached length changes only after success.
- The iterator owns a `BufferedReader` for its lifetime and yields decoded
  owned values from borrowed rows. The reader cache persists across iterator
  calls, avoiding per-record row-buffer allocation and reducing file reads to
  cache-window loads. Iteration stops after the first read or decode error.

## Testing and acceptance criteria

### Tests

Use **⏳ Not run** until a test has been executed, then set its status to
**✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `RSF-SF-FR-001`, `RSF-SF-RS-001` | Construct a `StructFile` from a `FileMetadata` with a matching row width. | Construction succeeds and owns the metadata; the caller can recover it after consuming the wrapper. | `rust_serial_file/tests/struct_file.rs` | `constructs_from_file_metadata` | ✅ Success |
| T-002 | `RSF-SF-FR-002`, `RSF-SF-FR-003` | Construct with zero-sized and mismatched serialized row widths. | Construction fails before any row operation or file mutation. | `rust_serial_file/tests/struct_file.rs` | `rejects_invalid_serialized_row_width` | ✅ Success |
| T-003 | `RSF-SF-FR-003` | Open/create a typed file with a metadata prefix. | The row width is `T::SIZE`, the offset is retained, and an absent path is created. | `rust_serial_file/tests/struct_file.rs` | `opens_or_creates_typed_file` | ✅ Success |
| T-004 | `RSF-SF-FR-004`, `RSF-SF-DS-003`, `RSF-SF-DS-004` | Write and read typed values at existing row indices. | Values round-trip and exactly one `T::SIZE` row is read or written. | `rust_serial_file/tests/struct_file.rs` | `reads_and_replaces_typed_values` | ✅ Success |
| T-005 | `RSF-SF-FR-004`, `RSF-SF-DS-003` | Use a `SerializedStruct` implementation that reports an incorrect encoded length. | The operation returns an error and does not write the malformed row. | `rust_serial_file/tests/struct_file.rs` | `rejects_incorrect_encoded_length` | ✅ Success |
| T-006 | `RSF-SF-FR-005`, `RSF-SF-RS-002` | Iterate all typed rows from a file. | Values are yielded in row order as owned results and iteration terminates after the captured range. | `rust_serial_file/tests/struct_file.rs` | `iterates_typed_rows_in_order` | ✅ Success |
| T-007 | `RSF-SF-FR-005` | Cause a row read failure during iteration. | The failure is yielded as an error and iteration terminates. | `rust_serial_file/tests/struct_file.rs` | `iterator_reports_row_errors` | ✅ Success |
| T-008 | `RSF-SF-FR-006`, `RSF-SF-DS-006` | Append one or more values through `SerialDataWriter`. | File rows and `serial_len` increase consistently after successful appends. | `rust_serial_file/tests/struct_file.rs` | `serial_write_appends_values` | ✅ Success |
| T-009 | `RSF-SF-FR-006`, `RSF-SF-FR-008` | Insert values at the beginning, middle, and end, and try an out-of-range index. | Existing records shift without changing order; end insertion appends; invalid insertion fails without changing contents. | `rust_serial_file/tests/struct_file.rs` | `serial_insert_shifts_rows_and_checks_bounds` | ✅ Success |
| T-010 | `RSF-SF-FR-006` | Replace an existing value and attempt replacement at `serial_len()`. | The existing row is updated; the one-past-end replacement returns an error. | `rust_serial_file/tests/struct_file.rs` | `serial_write_at_replaces_existing_value` | ✅ Success |
| T-011 | `RSF-SF-FR-006`, `RSF-SF-FR-009` | Remove first, middle, and last values, with and without returning the removed value. | Removed values are correct, remaining order is preserved, length decreases, and invalid indices fail. | `rust_serial_file/tests/struct_file.rs` | `serial_remove_preserves_remaining_order` | ✅ Success |
| T-012 | `RSF-SF-FR-006`, `RSF-SF-FR-010`, `RSF-SF-DS-005` | Clear a file with metadata and truncate a collection to shorter, equal, and larger lengths. | Rows are removed or retained according to trait semantics and metadata bytes remain unchanged. | `rust_serial_file/tests/struct_file.rs` | `serial_clear_and_truncate_preserve_metadata` | ✅ Success |
| T-013 | `RSF-SF-FR-007`, `RSF-SF-DS-008` | Use container search, update, and sorted insertion on ordered test records. | Trait defaults operate on file-backed records and preserve ordering. | `rust_serial_file/tests/struct_file.rs` | `container_operations_use_typed_records` | ✅ Success |
| T-014 | `RSF-SF-FR-012`, `RSF-SF-RS-005` | Query length and emptiness before and after successful and failed operations. | Length queries require no file I/O and change only after successful mutations. | `rust_serial_file/tests/struct_file.rs` | `maintains_infallible_collection_length` | ✅ Success |
| T-015 | `RSF-SF-FR-011` | Access and recover the owned `FileMetadata`. | The immutable accessor does not permit mutation, and consuming recovery returns the metadata owner. | `rust_serial_file/tests/struct_file.rs` | `exposes_and_recovers_owned_metadata` | ✅ Success |
| T-016 | `RSF-SF-DS-001`, `RSF-SF-DS-005`, `RSF-SF-DS-007` | Reopen an existing typed file with a metadata prefix and verify all rows. | Prefix bytes and row order remain unchanged, and no additional file-format header is introduced. | `rust_serial_file/tests/struct_file.rs` | `preserves_existing_file_layout` | ✅ Success |
| T-017 | `RSF-SF-FR-013`, `RSF-SF-DS-009` | Bulk-write a pre-ordered slice shorter than, equal to, and longer than the existing row count, including an empty slice. | The slice is written from row zero in its original order, excess values append contiguously, untouched trailing rows remain, and an empty slice does not mutate the file. | `rust_serial_file/tests/struct_file.rs` | `write_multiple_replaces_and_appends_in_slice_order` | ✅ Success |
| T-018 | `RSF-SF-FR-006`, `RSF-SF-DS-006` | Make `try_encode` fail during append and bulk write. | Errors are returned, file bytes are unchanged, and the cached length does not increase. | `rust_serial_file/tests/struct_file.rs` | `encode_errors_do_not_change_collection_length` | ✅ Success |
| T-019 | `RSF-SF-FR-004`, `RSF-SF-FR-005`, `RSF-SF-FR-009` | Make `try_decode` fail during direct read, removal, and iteration. | Errors are returned, removal leaves the file-backed collection unchanged, and iteration terminates after its error. | `rust_serial_file/tests/struct_file.rs` | `decode_errors_are_reported_without_removing_rows` | ✅ Success |
| T-020 | `RSF-SF-FR-009` | Remove a row from a collection large enough to require multiple shift-buffer chunks. | Rows spanning chunk boundaries retain their order and the logical length decreases correctly. | `rust_serial_file/tests/struct_file.rs` | `serial_remove_shifts_rows_across_multiple_chunks` | ✅ Success |
| T-021 | `RSF-SF-RS-006` | Request an impossible row-buffer allocation. | The allocation error is propagated. | `rust_serial_file/src/file/struct_file.rs` | `reports_row_buffer_allocation_failure` | ✅ Success |
| T-022 | `RSF-SF-DS-004` | Read a row larger than the fixed stack buffer. | The larger row is read and decoded correctly using the fallible allocation path. | `rust_serial_file/tests/struct_file.rs` | `reads_rows_larger_than_the_stack_buffer` | ✅ Success |
| T-023 | `RSF-SF-FR-005`, `RSF-SF-RS-002` | Iterate more rows than fit in the BufferedReader cache. | All rows are decoded in order across cache-window boundaries. | `rust_serial_file/tests/struct_file.rs` | `iterator_reads_across_buffer_cache_windows` | ✅ Success |
| T-024 | `RSF-SF-FR-006`, `RSF-SF-RS-006` | Replace, append, and insert a record larger than the stack buffer. | Large-record writes use the fallible allocation path and preserve row order and values. | `rust_serial_file/tests/struct_file.rs` | `writes_rows_larger_than_the_stack_buffer` | ✅ Success |
| T-025 | `RSF-SF-FR-004`, `RSF-SF-FR-006`, `RSF-SF-FR-009` | Replace and remove-and-return at the current collection length. | Both methods return errors without changing row bytes or logical length. | `rust_serial_file/tests/struct_file.rs` | `rejects_out_of_range_replace_and_remove_get_without_mutation` | ✅ Success |

### Acceptance criteria

1. `StructFile<T>` owns a `FileMetadata`, validates its row width against
   `T::SIZE`, and is exported through the file module.
2. Typed reads, writes, and iteration use the `SerializedStruct` fallible
   APIs and report malformed serialization and file errors.
3. `write_multiple` writes records from row zero in caller-supplied order,
   overwrites the existing prefix, and appends any excess suffix without
   sorting or validating record ordering.
4. The type implements `SerialDataReader<T>`, `SerialDataWriter<T>`, and
   `SerialDataContainer<T>` with correct indexing, ordering, and length
   semantics.
5. Insertions and removals preserve the order of unaffected records and the
   metadata prefix.
6. Boundary and failure behavior is tested; applicable formatting, lint,
   build, and test checks pass.
7. Coverage is measured for the complete `StructFile` production module and
   uncovered reachable behavior is tested or documented.

## Code coverage

Record the coverage tool, measured production-module scope, coverage
percentage, and any remaining uncovered behavior after implementation. Do not
exclude relevant production code to increase the reported percentage.

- Tool: `cargo llvm-cov --all-targets --summary-only`.
- Scope: complete `src/file/struct_file.rs` production module, including
  integration and module unit tests.
- **Coverage summary:** 100.00% function coverage, 94.47% line coverage, and
  82.65% region coverage.
- `cargo llvm-cov --show-missing-lines` reports no specific uncovered
  `StructFile` source lines; the remaining missed coverage is in regions not
  identified as uncovered source lines.

## Out of scope

- Defining or changing the `SerializedStruct` wire format or derive macro.
- Changing `FileMetadata`, `SerialDataReader`, `SerialDataWriter`, or
  `SerialDataContainer` behavior.
- Concurrent mutation through multiple file handles or processes.
- Transactional rollback for partial file writes or row shifts.
- Automatic sorting except where explicitly requested through the sorted
  `SerialDataContainer` operations.
- Schema evolution, type identification, version headers, checksums, and
  migration of existing files.
- Returning mutable references to records stored in the file; records are
  read as owned decoded values.
