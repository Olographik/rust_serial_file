# Serial Read Container Requirements

- **Date:** 2026-11-07
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Define a shared read interface for serial collections backed by in-memory containers, files, databases, and other storage.

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
- [Testing and acceptance criteria](#testing-and-acceptance-criteria)
- [Code coverage](#code-coverage)
- [Out of scope](#out-of-scope)

## Overview

Define `SerialDataReader<T>`, the read portion of a serial collection
interface. A serial collection may be backed by an in-memory vector or map, a
file, a SQL database, or another storage system. Callers use the same
collection-oriented read operations without depending on the storage backend.

All interface read-operation names use the `serial_` prefix to avoid naming
collisions with backend methods. The existing
`src/adapter/abstract_data_container.rs` draft is a reference for collection
semantics and naming, not a requirement to include its write operations in this
read interface.

Reading may change backend state. For example, a file-backed implementation
may refill or advance a read buffer when a record is accessed. The interface
and its implementations must permit such state updates while preserving the
observable collection semantics.

## Goals and scope

### Goals

- Define a backend-neutral read interface for serial collections of `T`.
- Provide named operations for reading a record by index, the first or last
  record, and records in collection order.
- Ensure operations support backends whose reads update internal state, such
  as file buffers or database cursors.
- Define consistent collection, indexing, ordering, and failure semantics for
  all implementations.

### Scope

- Specify the public reader trait and its read-only collection operations.
- Cover in-memory, file-backed, map-like, and database-backed implementations
  as possible backends.
- Preserve the collection's logical length and element order during reads.
- Exclude mutation and persistence operations; those belong to a separate
  writer or collection interface.
- Keep the existing `SerialDataContainer<T>` draft and its write operations
  outside the scope of this reader contract unless a later design explicitly
  composes the interfaces.

## Assumptions and dependencies

- The reader represents an ordered logical sequence of records addressable by
  zero-based indices, even when the underlying storage is not itself a
  sequence.
- The caller and implementation agree on how a backend maps its stored values
  to the logical record order.
- Some backends can read without changing state; others may update internal
  buffer, cache, cursor, or position state while servicing a read.
- The reader API uses `anyhow::Result` so it can represent failures from
  different storage backends through one interface. The crate must depend on
  `anyhow`.
- The exact Rust edition and MSRV are not established by this requirements
  document. These must follow the crate's project-wide decisions.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| RSF-FR-001 | The interface shall expose a public `SerialDataReader<T>` trait representing the read operations of a serial collection. |
| RSF-FR-002 | Every read operation introduced by the interface shall have a `serial_`-prefixed name. In particular, the indexed read operation shall use a name such as `serial_get`, not an unqualified `get` or `read`. |
| RSF-FR-003 | An indexed read shall address records by zero-based index and return an owned record as `anyhow::Result<T>`. An index outside the logical collection shall produce a distinguishable not-found error. |
| RSF-FR-004 | The interface shall provide operations to read the first and last record as `anyhow::Result<T>`. An empty collection shall produce a distinguishable not-found error. |
| RSF-FR-005 | The interface shall provide an iteration operation that reads owned records in logical collection order and makes per-record backend failures observable. |
| RSF-FR-006 | The interface shall expose logical length and empty state through infallible `serial_`-prefixed operations, `serial_len(&self) -> usize` and `serial_is_empty(&self) -> bool`. Implementations shall make this metadata available without fallible backend I/O, for example by loading it during construction and maintaining it as the collection changes. |
| RSF-FR-007 | Read methods shall be allowed to mutate backend state as required to complete a read. Such state updates may include loading a file buffer, advancing or repositioning a cursor, or updating a cache. They shall not change the logical values or order exposed by the collection. |
| RSF-FR-008 | Repeated reads of a record without an intervening collection change shall return the same logical value, regardless of internal buffering or cursor movement. |
| RSF-FR-009 | Iteration shall visit each logical record once, in order, and shall not skip or duplicate records when the implementation updates read state. |
| RSF-FR-010 | Implementations shall define and consistently apply behavior for reading after the end of the collection and for an empty collection. |
| RSF-FR-011 | The interface shall provide default binary-search operations: `serial_find(&T)` for a value, `serial_find_by<K>(&K)` for a key where `T: PartialOrd<K>`, and `serial_find_by_ordering` with a comparator receiving each record as `&T` and returning its ordering relative to the search target. Each returns `anyhow::Result<Option<usize>>`: `None` means no match, `Some(index)` means a match, and `Err` reports a backend read or comparison failure. |
| RSF-FR-012 | Each search form shall have a variant accepting `&mut SearchContext` and use the context-aware binary-search helper. A `serial_find_and_get` extension shall return `anyhow::Result<Option<T>>`, where `Ok(None)` means no match, `Ok(Some(value))` means a record was found and read, and `Err(_)` reports a search or record-read failure. |
| RSF-FR-013 | Binary-search operations require the collection to be ordered according to the comparison used by that operation. The reader shall document this precondition; search does not sort or otherwise modify records. |

### Data and serialization

| ID | Requirement |
|---|---|
| RSF-DS-001 | The interface is generic over the record type `T`; it does not prescribe a serialized representation or require `T` to implement a particular serialization trait. |
| RSF-DS-002 | A backend that decodes or maps stored data to `T` is responsible for preserving its own format and record-boundary rules. |
| RSF-DS-003 | Reads shall not modify stored data as a side effect. Internal read state updates, such as buffer and cursor changes, are permitted. |
| RSF-DS-004 | The backend shall not present incomplete or invalid stored data as a valid record. Its handling of malformed data must be documented and failures observable. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| RSF-RS-001 | Read operations shall return owned records rather than references into backend storage. Iterator lifetimes shall still be bounded by the reader when iteration borrows reader state. |
| RSF-RS-002 | Read methods shall permit mutable access to the reader when backend read state must be updated. |
| RSF-RS-003 | Indexed and endpoint reads shall return `anyhow::Result<T>`. Iterator items shall be `anyhow::Result<T>` so failures encountered after iteration begins are observable. |
| RSF-RS-004 | Implementations shall not require unnecessary trait bounds on `T`; bounds shall be limited to operations that actually need them. An implementation for `Vec<T>` shall require `T: Clone` to return owned records. |
| RSF-RS-005 | The associated iterator type shall be usable by generic callers through the trait and shall yield `anyhow::Result<T>` items. |
| RSF-RS-006 | Value search shall require `T: PartialOrd<T>`. Key search shall require `T: PartialOrd<K>` and permit `K` to differ from `T`. Comparator search shall accept a callback of the form `FnMut(&T) -> Ordering`, with the callback comparing each record to the captured search target. |
| RSF-RS-007 | Value and key searches whose `PartialOrd::partial_cmp` comparison is undefined shall report an error rather than panic or fabricate a match. |
| RSF-RS-008 | `serial_len` and `serial_is_empty` shall be infallible and shall not perform fallible backend I/O. Implementations shall initialize required metadata before exposing the reader and keep it available for these methods. |

## Public API

- **Trait:** `SerialDataReader<T>`.
- **Required read operations:**
  - `serial_get(&mut self, index: usize) -> anyhow::Result<T>` for indexed
    reads. An invalid index returns a not-found error.
  - `serial_first(&mut self) -> anyhow::Result<T>` and
    `serial_last(&mut self) -> anyhow::Result<T>`. An empty collection returns
    a not-found error.
  - `serial_iter(&mut self) -> anyhow::Result<Self::Iter<'_>>`, where
    `Self::Iter<'a>: Iterator<Item = anyhow::Result<T>>`. Iterator
    construction and per-record reads can therefore report failures.
  - `serial_len(&self) -> usize` and `serial_is_empty(&self) -> bool`. These
    methods shall use metadata already available to the reader and shall not
    perform fallible backend I/O.
  - `serial_find(&mut self, value: &T) -> anyhow::Result<Option<usize>>`
    for a value.
  - `serial_find_by<K>(&mut self, key: &K) -> anyhow::Result<Option<usize>>`
    where `T: PartialOrd<K>`.
  - `serial_find_by_ordering<F>(&mut self, comparator: F) ->
    anyhow::Result<Option<usize>>` where `F: FnMut(&T) -> Ordering`. The
    comparator shall return the ordering of each visited record relative to
    the captured search target.
  - Context-aware counterparts `serial_find_with_context`,
    `serial_find_by_with_context`, and
    `serial_find_by_ordering_with_context`, each accepting
    `&mut SearchContext`.
  - A `serial_find_and_get` extension returning
    `anyhow::Result<Option<T>>` for a value/key search: `Ok(None)` means no
    match; `Ok(Some(value))` means a record was found and read; `Err(error)`
    reports a failure during search or record retrieval.
- **State access:** Read operations may require `&mut self` so implementations
  can update buffer, cache, or cursor state.
- **Collection metadata:** The reader shall have the logical length available
  without a fallible read, so `serial_len` and `serial_is_empty` can be
  infallible. Backends shall load or establish this metadata during their
  fallible construction/opening step and keep it consistent while the reader
  is in use.
- **Fallibility:** All backend operations that may fail shall report failures
  using `anyhow::Result`. Not-found conditions (including an out-of-range
  index or an empty endpoint read) shall be distinguishable from backend
  failures, for example through a typed error carried by `anyhow::Error`.
- **Search defaults:** Default find operations shall use the project's
  `dicotomic_search_fn` and `dicotomic_search_with_context_fn` helpers (or
  equivalent error-aware adapters using the same binary-search algorithm),
  adapted as necessary to preserve fallible backend reads. Search errors
  shall not be unwrapped, discarded, or converted to `None`.
- **Search context:** `SearchContext` is the project type from
  `utils::search::search_type`; context-aware methods shall preserve its
  update behavior as records are compared.
- **Composition:** Whether `SerialDataReader<T>` is standalone or is composed
  into a larger serial collection trait is a design decision. Reader-only
  implementations shall not be forced to expose mutation operations.
- Associated type declarations and any necessary helper types shall be agreed
  during API design.

## Errors and failure behavior

- An index outside the logical collection and an empty endpoint read shall
  return distinguishable not-found errors, separate from backend failures.
- File, database, decoding, metadata, iterator-construction, and per-record
  iteration failures shall be observable through `anyhow::Result`.
- Failures encountered while fetching records for a search shall be returned
  as errors. A `serial_find_and_get` implementation shall represent such an
  error as `Err(error)`, as specified by its return shape.
- `serial_len` and `serial_is_empty` shall not fail: any backend work required
  to establish their metadata shall occur before those methods are called.
  Initialization failures shall be reported by the reader's fallible
  construction/opening operation. Implementations shall not silently replace
  unknown metadata with a success-shaped value.
- If a `PartialOrd` comparison has no ordering, the search shall report an
  error rather than panic or claim that a record matched.
- A failed read shall not be silently represented as a successful empty
  result, a default value, a stale record, or end-of-iteration.
- If an iteration fails after yielding records, the relevant iterator item
  shall contain the error rather than report normal completion.
- State changes made before a failed read shall leave the reader in a
  documented state from which subsequent operations behave predictably.

## Safety and security

- `unsafe` code is not required by these requirements. Any use of `unsafe`
  must be justified and reviewed according to project policy.
- Implementations shall validate backend-derived indices, lengths, offsets,
  and record boundaries before using them to access memory or storage.
- Database and file implementations shall handle malformed or untrusted
  stored data without panics, memory unsafety, or success-shaped fallback
  values.
- Resource handles, file paths, query details, and stored values shall not be
  exposed in errors beyond what is necessary for callers to diagnose failures.

## Compatibility

- The trait shall use the `SerialDataReader<T>` name and `serial_`-prefixed
  read-operation naming to avoid collisions with backend APIs.
- The existing `SerialDataContainer<T>` draft in
  `src/adapter/abstract_data_container.rs` is a source of naming and
  collection-semantics guidance. This work shall not silently change its
  existing behavior or imply that its mutation methods are part of the
  reader-only API.
- Implementations for vectors, maps, files, SQL, or other backends shall
  expose consistent logical read semantics even when their storage and
  buffering strategies differ.
- Changes to existing public APIs or serialized formats are not required by
  this document. Any such change requires a separately documented
  compatibility decision.

## Performance and resource use

- Implementations may buffer, cache, or maintain backend cursors to serve
  reads efficiently.
- Read operations return owned `T` values. Implementations may need to clone
  values held in memory; the `Vec<T>` implementation therefore requires
  `T: Clone`. Persistent backends may construct owned values while decoding.
- Buffering and cursor updates shall not alter the logical results or ordering
  of reads.
- No fixed latency, throughput, memory, or buffer-capacity guarantee is
  imposed here. Backend-specific performance constraints shall be documented
  with the corresponding implementation.
- Concurrency and synchronization guarantees are backend-specific and must be
  documented by each implementation.

## Phases

- [x] Requirements clarified and accepted.
- [x] Design agreed, including `anyhow::Result` error classification,
  iterator behavior, search signatures and helpers, metadata availability,
  and trait composition.
- [x] Implementation completed.
- [x] Tests added and passing.
- [ ] Documentation updated and changes reviewed.

## Testing and acceptance criteria

### Tests

The table records tests implemented in `src/adapter/abstract_serial_read.rs`.
Tests are marked **✅ Success** only after execution; any outstanding tests
remain **⏳ Not run**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | RSF-FR-001, RSF-FR-002, RSF-FR-003, RSF-FR-004, RSF-FR-005, RSF-FR-006, RSF-FR-010, RSF-RS-008 | Verify `Vec<T>` provides owned indexed, endpoint, and iteration reads, infallible length/empty queries, and not-found errors. | Reads return the expected cloned values in order; an invalid index and empty first read report the corresponding typed errors. | `src/adapter/abstract_serial_read.rs` | `vector_reader_returns_owned_records_and_reports_invalid_indices` | ✅ Success |
| T-002 | RSF-FR-011, RSF-FR-012, RSF-FR-013, RSF-RS-006 | Verify value, key, and comparator searches, a key search with context, and both value/key find-and-get operations. | Matching searches return the expected index or value; missing keys return `None`; context is updated to the found index. | `src/adapter/abstract_serial_read.rs` | `reader_searches_by_value_key_and_comparator` | ✅ Success |
| T-003 | RSF-RS-007 | Verify key search reports undefined `PartialOrd` ordering. | Searching values containing `NaN` returns the typed `UndefinedOrdering` error rather than panicking. | `src/adapter/abstract_serial_read.rs` | `undefined_partial_ordering_is_reported` | ✅ Success |
| T-004 | RSF-FR-007, RSF-FR-008, RSF-FR-009 | Verify reads and iteration across backend buffer state changes. | Refills do not change values, skip records, duplicate records, or reorder iteration. | `src/adapter/abstract_serial_read.rs` | `buffered_reads_preserve_records_across_refills_and_iteration` | ✅ Success |
| T-005 | RSF-FR-003, RSF-FR-005, RSF-FR-011, RSF-FR-012 | Verify backend failures during indexed reads, searches, iterator creation, and per-item iteration. | Each failure remains observable as an error and is not converted into not-found or normal iteration completion. | `src/adapter/abstract_serial_read.rs` | `backend_read_and_iterator_errors_remain_observable` | ✅ Success |
| T-006 | RSF-FR-012 | Verify context-aware searches and find-and-get found, no-match, and error outcomes. | Context-aware methods return the expected result and context; find-and-get returns `Ok(Some(value))`, `Ok(None)`, or `Err` as appropriate. | `src/adapter/abstract_serial_read.rs` | `context_search_and_find_and_get_report_all_outcomes` | ✅ Success |
| T-007 | RSF-FR-001, RSF-FR-002, RSF-FR-005, RSF-FR-006 | Verify an alternate map-backed reader implements the shared contract through a generic caller. | Generic read code works without relying on backend-specific method names and reads records in logical order. | `src/adapter/abstract_serial_read.rs` | `generic_read_client_supports_an_alternate_backend` | ✅ Success |

**Executed results:** `cargo test --manifest-path rust_serial_file\Cargo.toml
adapter::abstract_serial_read::tests` passed all 7 reader tests. The full
`cargo test --manifest-path rust_serial_file\Cargo.toml` run passed all 24
tests, with 0 failures; doc-tests also passed (0 tests).

### Acceptance criteria

1. The public reader interface and its `serial_`-prefixed read operations are
   documented and agreed.
2. Indexed, endpoint, and iteration reads have consistent empty, ordering,
   and out-of-range semantics.
3. Implementations can update internal read state without changing the
   logical results visible to callers.
4. Reads return owned values through `anyhow::Result`; not-found conditions
   are distinguishable from backend failures, including failures during
   iteration.
5. Length and empty-state queries are infallible and rely only on metadata
   available to the reader without fallible backend I/O.
6. Default value, key, and comparator searches use binary search, support
   `SearchContext`, and propagate backend and comparison failures.
7. Binary-search operations document and rely on the collection's relevant
   sorted-order precondition without mutating records.
8. `serial_find_and_get` distinguishes no match, a successfully read record,
    and search/read failure according to its `anyhow::Result<Option<T>>`
    return shape.
9. Tests verify the required behavior for both a non-stateful in-memory
    backend and a stateful buffered or cursor-based backend. The current tests
    cover the in-memory vector, a buffered reader double, and an alternate
    map-backed reader.
10. Existing `SerialDataContainer<T>` behavior remains unchanged unless
    changes are separately agreed and documented.

## Code coverage

Coverage has not been measured. Test pass counts above are not coverage
results.

- **Coverage summary:** Not measured; no coverage tool or percentage was
  recorded.

## Out of scope

- Collection mutation, writing, persistence, and flushing.
- A specific file, map, SQL, or other backend implementation.
- A particular serialization format or schema.
- A fixed buffering strategy or performance guarantee.
- Whether the reader is composed into a broader collection trait; resolve
  this during API design.
