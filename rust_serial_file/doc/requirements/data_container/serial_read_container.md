# Serial Read Container Requirements

- **Date:** 2026-11-11
- **Author:** `Olographik`
- **Revision:** 0.2
- **Short description:** Define a shared, shared-reference (`&self`) read interface for serial collections backed by in-memory containers, files, databases, and other storage.

## Table of contents

- [Overview](#overview)
- [Revisions](#revisions)
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
may refill a read buffer, and a database-backed implementation may reuse a
cached prepared statement. Revision 0.1 exposed reads through `&mut self` so
backends could update that state directly. Revision 0.2 changes every read
operation to take `&self`: reading is logically non-mutating, so a reader can
be shared through `&R`, held next to other shared borrows, and placed behind a
reader/writer lock. Backends that must update internal read state do so
through interior mutability (`Cell`, `RefCell`, `Mutex`, or a backend API that
is already `&self`, such as `FileMetadata::read_row` or rusqlite's
`Connection::prepare_cached`). Write operations stay on `&mut self` in
`SerialDataWriter<T>`.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-11 | `Olographik` | Read operations change from `&mut self` to `&self`. Modified: `RSF-FR-007` (state updates go through interior mutability), `RSF-RS-002` (reads take `&self`). Added: `RSF-RS-009` (interior mutability for read state), `RSF-RS-010` (overlapping reads and borrow conflicts), `RSF-RS-011` (thread-safety bounds are backend-specific). Public API, Compatibility, and Performance sections updated to match. Document migrated to the current requirements template: Revisions section, Revision column on requirement tables, phase IDs, split test tables, and the coverage results table. Tests T-013 to T-015 added. |

## Goals and scope

### Goals

- Define a backend-neutral read interface for serial collections of `T`.
- Provide named operations for reading a record by index, the first or last
  record, and records in collection order.
- Expose every read operation through a shared reference (`&self`), so a
  reader can be read from wherever a `&R` is available.
- Support backends whose reads update internal state, such as file buffers,
  statement caches, or database cursors, through interior mutability.
- Define consistent collection, indexing, ordering, and failure semantics for
  all implementations.

### Scope

- Specify the public reader trait and its read-only collection operations.
- Cover in-memory, file-backed, map-like, and database-backed implementations
  as possible backends.
- Update the in-crate implementors to the `&self` signatures: `Vec<T>` in
  `src/adapter/abstract_serial_read.rs`, `StructFile<T>` in
  `src/file/struct_file.rs`, the test doubles in
  `src/adapter/abstract_serial_read.rs` and
  `src/adapter/abstract_data_container.rs`, and the internal
  `find_insert_position` helper used by `SerialDataContainer<T>`.
- Preserve the collection's logical length and element order during reads.
- Exclude mutation and persistence operations; those belong to
  `SerialDataWriter<T>`, which keeps its `&mut self` receivers.
- Keep the behavior of `SerialDataContainer<T>` and its default methods
  unchanged; only the borrow it needs to call reader methods becomes weaker.

## Assumptions and dependencies

- The reader represents an ordered logical sequence of records addressable by
  zero-based indices, even when the underlying storage is not itself a
  sequence.
- The caller and implementation agree on how a backend maps its stored values
  to the logical record order.
- Some backends can read without changing state; others update internal
  buffer, cache, cursor, or position state while servicing a read. Since
  revision 0.2, the latter use interior mutability.
- The file backend is already compatible: `FileMetadata::read_row` and
  `FileMetadata::read_rows` take `&self`, and `BufferedReader` borrows
  `&FileMetadata`.
- A SQLite backend built on rusqlite is expected to be compatible:
  `Connection::prepare`, `Connection::prepare_cached`, `Connection::query_row`,
  and `Connection::unchecked_transaction` take `&self`. Only `Statement`
  query methods need `&mut`, on a statement created for the read, not on the
  reader. `Connection` is `Send` but not `Sync`.
- The reader API uses `anyhow::Result` so it can represent failures from
  different storage backends through one interface. The crate depends on
  `anyhow` 1.0.
- Dependencies: `anyhow`, the workspace `utils` crate (for `SearchContext`
  and `SearchIndexes`), and `tempfile` for tests.
- The crate uses Rust edition 2024. No MSRV is set by this document; it
  follows the crate's project-wide decisions. Generic associated types
  (`type Iter<'a>`) require Rust 1.65 or later.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `RSF-FR-001` | The interface shall expose a public `SerialDataReader<T>` trait representing the read operations of a serial collection. | 0.1 |
| `RSF-FR-002` | Every read operation introduced by the interface shall have a `serial_`-prefixed name. In particular, the indexed read operation shall use a name such as `serial_get`, not an unqualified `get` or `read`. | 0.1 |
| `RSF-FR-003` | An indexed read shall address records by zero-based index and return an owned record as `anyhow::Result<T>`. An index outside the logical collection shall produce a distinguishable not-found error. | 0.1 |
| `RSF-FR-004` | The interface shall provide operations to read the first and last record as `anyhow::Result<T>`. An empty collection shall produce a distinguishable not-found error. | 0.1 |
| `RSF-FR-005` | The interface shall provide an iteration operation that reads owned records in logical collection order and makes per-record backend failures observable. | 0.1 |
| `RSF-FR-006` | The interface shall expose logical length and empty state through infallible `serial_`-prefixed operations, `serial_len(&self) -> usize` and `serial_is_empty(&self) -> bool`. Implementations shall make this metadata available without fallible backend I/O, for example by loading it during construction and maintaining it as the collection changes. | 0.1 |
| `RSF-FR-007` | Read operations shall be allowed to update internal backend state as required to complete a read, such as loading a file buffer, advancing or repositioning a cursor, or updating a cache. Because reads take `&self`, such state shall be held behind interior mutability or obtained from a backend API that already accepts a shared reference. State updates shall not change the logical values or order exposed by the collection. | 0.2 |
| `RSF-FR-008` | Repeated reads of a record without an intervening collection change shall return the same logical value, regardless of internal buffering or cursor movement. | 0.1 |
| `RSF-FR-009` | Iteration shall visit each logical record once, in order, and shall not skip or duplicate records when the implementation updates read state. | 0.1 |
| `RSF-FR-010` | Implementations shall define and consistently apply behavior for reading after the end of the collection and for an empty collection. | 0.1 |
| `RSF-FR-011` | The interface shall provide default binary-search operations: `serial_find(&T)` for a value, `serial_find_by<K>(&K)` for a key where `T: PartialOrd<K>`, and `serial_find_by_ordering` with a comparator receiving each record as `&T` and returning its ordering relative to the search target. Each returns `anyhow::Result<Option<usize>>`: `None` means no match, `Some(index)` means a match, and `Err` reports a backend read or comparison failure. | 0.1 |
| `RSF-FR-012` | Each search form shall have a variant accepting `&mut SearchContext` and use the context-aware binary-search helper. A `serial_find_and_get` extension shall return `anyhow::Result<Option<T>>`, where `Ok(None)` means no match, `Ok(Some(value))` means a record was found and read, and `Err(_)` reports a search or record-read failure. | 0.1 |
| `RSF-FR-013` | Binary-search operations require the collection to be ordered according to the comparison used by that operation. The reader shall document this precondition; search does not sort or otherwise modify records. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `RSF-DS-001` | The interface is generic over the record type `T`; it does not prescribe a serialized representation or require `T` to implement a particular serialization trait. | 0.1 |
| `RSF-DS-002` | A backend that decodes or maps stored data to `T` is responsible for preserving its own format and record-boundary rules. | 0.1 |
| `RSF-DS-003` | Reads shall not modify stored data as a side effect. Internal read state updates, such as buffer and cursor changes, are permitted. | 0.1 |
| `RSF-DS-004` | The backend shall not present incomplete or invalid stored data as a valid record. Its handling of malformed data must be documented and failures observable. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `RSF-RS-001` | Read operations shall return owned records rather than references into backend storage. Iterator lifetimes shall still be bounded by the reader when iteration borrows reader state. | 0.1 |
| `RSF-RS-002` | Every read operation of `SerialDataReader<T>` (`serial_len`, `serial_is_empty`, `serial_get`, `serial_first`, `serial_last`, `serial_iter`, and every `serial_find*` operation) shall take `&self`. No read operation shall require `&mut self`. Only caller-owned state passed as an argument, such as `&mut SearchContext`, may be mutably borrowed. | 0.2 |
| `RSF-RS-003` | Indexed and endpoint reads shall return `anyhow::Result<T>`. Iterator items shall be `anyhow::Result<T>` so failures encountered after iteration begins are observable. | 0.1 |
| `RSF-RS-004` | Implementations shall not require unnecessary trait bounds on `T`; bounds shall be limited to operations that actually need them. An implementation for `Vec<T>` shall require `T: Clone` to return owned records. | 0.1 |
| `RSF-RS-005` | The associated iterator type shall be usable by generic callers through the trait and shall yield `anyhow::Result<T>` items. | 0.1 |
| `RSF-RS-006` | Value search shall require `T: PartialOrd<T>`. Key search shall require `T: PartialOrd<K>` and permit `K` to differ from `T`. Comparator search shall accept a callback of the form `FnMut(&T) -> Ordering`, with the callback comparing each record to the captured search target. | 0.1 |
| `RSF-RS-007` | Value and key searches whose `PartialOrd::partial_cmp` comparison is undefined shall report an error rather than panic or fabricate a match. | 0.1 |
| `RSF-RS-008` | `serial_len` and `serial_is_empty` shall be infallible and shall not perform fallible backend I/O. Implementations shall initialize required metadata before exposing the reader and keep it available for these methods. | 0.1 |
| `RSF-RS-009` | An implementation that updates internal state during a read shall hold that state behind interior mutability chosen by the backend: `Cell` or `RefCell` for single-threaded readers, or a synchronization primitive such as `Mutex` or `RwLock` for readers that must be `Sync`. The trait itself shall not prescribe the mechanism. | 0.2 |
| `RSF-RS-010` | Several read operations may be in progress at once on one reader, for example a live iterator from `serial_iter` while `serial_get` or a search is called. An implementation shall either support such overlapping reads with correct results or report the conflict as an `Err`; it shall never panic (for example on a `RefCell` borrow conflict) or deadlock. Interior borrows shall be scoped to a single operation or iterator step rather than held between calls. | 0.2 |
| `RSF-RS-011` | The trait shall not require `Self: Send` or `Self: Sync`. Each implementation shall document whether it is `Send` and `Sync`. A backend that wraps a non-`Sync` handle (such as a rusqlite `Connection`) may remain non-`Sync`, or wrap the handle in a synchronization primitive to become `Sync`. | 0.2 |

## Public API

- **Trait:** `SerialDataReader<T>` in `src/adapter/abstract_serial_read.rs`,
  together with the `SerialDataReaderError` enum (`IndexOutOfBounds`,
  `EmptyCollection`, `UndefinedOrdering`).
- **Associated type:** `type Iter<'a>: Iterator<Item = anyhow::Result<T>>
  where Self: 'a, T: 'a;`
- **Required read operations:**
  - `serial_len(&self) -> usize`. Uses metadata already available to the
    reader and performs no fallible backend I/O.
  - `serial_get(&self, index: usize) -> anyhow::Result<T>` for indexed reads.
    An invalid index returns a not-found error.
  - `serial_iter(&self) -> anyhow::Result<Self::Iter<'_>>`. Iterator
    construction and per-record reads can both report failures.
- **Provided read operations (default implementations):**
  - `serial_is_empty(&self) -> bool`.
  - `serial_first(&self) -> anyhow::Result<T>` and
    `serial_last(&self) -> anyhow::Result<T>`. An empty collection returns
    `SerialDataReaderError::EmptyCollection`.
  - `serial_find(&self, value: &T) -> anyhow::Result<Option<usize>>` where
    `T: PartialOrd<T>`.
  - `serial_find_by<K>(&self, key: &K) -> anyhow::Result<Option<usize>>`
    where `T: PartialOrd<K>`.
  - `serial_find_by_ordering<F>(&self, comparator: F) ->
    anyhow::Result<Option<usize>>` where `F: FnMut(&T) -> Ordering`. The
    comparator returns the ordering of each visited record relative to the
    captured search target.
  - Context-aware counterparts `serial_find_with_context(&self, value,
    context: &mut SearchContext)`, `serial_find_by_with_context(&self, key,
    context: &mut SearchContext)`, and
    `serial_find_by_ordering_with_context(&self, comparator, context: &mut
    SearchContext)`.
  - `serial_find_and_get(&self, value: &T) -> anyhow::Result<Option<T>>` and
    `serial_find_by_and_get<K>(&self, key: &K) -> anyhow::Result<Option<T>>`:
    `Ok(None)` means no match; `Ok(Some(value))` means a record was found and
    read; `Err(error)` reports a failure during search or record retrieval.
- **State access:** No read operation takes `&mut self`. Implementations that
  need to update buffer, cache, or cursor state use interior mutability
  (`RSF-RS-009`).
- **Implementations in this crate:** `Vec<T>` (requires `T: Clone`) and
  `StructFile<T>` (requires `T: SerializedStruct`).
- **Collection metadata:** The reader shall have the logical length available
  without a fallible read, so `serial_len` and `serial_is_empty` can be
  infallible. Backends load or establish this metadata during their fallible
  construction/opening step and keep it consistent while the reader is in
  use.
- **Fallibility:** All backend operations that may fail report failures using
  `anyhow::Result`. Not-found conditions (an out-of-range index or an empty
  endpoint read) are distinguishable from backend failures through the typed
  `SerialDataReaderError` carried by `anyhow::Error`.
- **Search defaults:** Default find operations use an error-aware binary
  search equivalent to the project's `dicotomic_search_fn` and
  `dicotomic_search_with_context_fn` helpers, so fallible backend reads are
  preserved. Search errors are not unwrapped, discarded, or converted to
  `None`.
- **Search context:** `SearchContext` is the project type from
  `utils::search::search_type`; context-aware methods preserve its update
  behavior as records are compared.
- **Composition:** `SerialDataReader<T>` is standalone. `SerialDataContainer<T>`
  in `src/adapter/abstract_data_container.rs` composes it with
  `SerialDataWriter<T>`; reader-only implementations are not forced to expose
  mutation operations.
- **Usage example (backend with read state):**

  ```rust
  struct CachedReader {
      records: Vec<i32>,
      cache: RefCell<Option<(usize, i32)>>,
  }

  impl SerialDataReader<i32> for CachedReader {
      // ...
      fn serial_get(&self, index: usize) -> anyhow::Result<i32> {
          let mut cache = self.cache.try_borrow_mut()
              .map_err(|_| anyhow::anyhow!("reader state is already borrowed"))?;
          // refill the cache if needed, then return an owned value
          // ...
      }
  }

  fn total<R: SerialDataReader<i32>>(reader: &R) -> anyhow::Result<i32> {
      reader.serial_iter()?.sum()
  }
  ```

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
- An interior-mutability conflict (an already-borrowed `RefCell`, a poisoned
  or contended lock that the implementation chooses not to wait on) shall be
  reported as an `Err` from the read operation, never as a panic or a
  success-shaped value (`RSF-RS-010`).
- State changes made before a failed read shall leave the reader in a
  documented state from which subsequent operations behave predictably. In
  particular, a partially refilled buffer shall be invalidated rather than
  served on a later read.

## Safety and security

- None: no `unsafe` code is required or allowed by these requirements. Any
  future use of `unsafe` (for example a hand-written `Sync` implementation)
  must be justified and reviewed according to project policy.
- Implementations shall validate backend-derived indices, lengths, offsets,
  and record boundaries before using them to access memory or storage.
- Database and file implementations shall handle malformed or untrusted
  stored data without panics, memory unsafety, or success-shaped fallback
  values.
- Resource handles, file paths, query details, and stored values shall not be
  exposed in errors beyond what is necessary for callers to diagnose failures.
- Implementations shall not use `unsafe` or `UnsafeCell` directly to obtain
  mutable access from `&self`; they shall use the safe standard-library cells
  and locks.

## Compatibility

- The trait keeps the `SerialDataReader<T>` name, the `serial_`-prefixed
  operation names, the return types, and the error types of revision 0.1.
- **Breaking change for implementors (revision 0.2):** `serial_get`,
  `serial_iter`, and any overridden provided method shall change their
  receiver from `&mut self` to `&self`. Implementors whose reads mutated
  fields directly must move that state behind interior mutability.
- **Callers are source-compatible:** code that calls reader methods through
  `&mut R` or an owned `R` still compiles, because method calls auto-borrow
  `&mut R` as `&R`. Generic helpers bounded by `R: SerialDataReader<T>` may
  relax their parameter from `&mut R` to `&R`; doing so is optional for
  downstream code.
- `SerialDataContainer<T>` default methods keep their `&mut self` receivers
  and observable behavior. The internal `find_insert_position` helper only
  needs `&C` after this change.
- `SerialDataWriter<T>` is unchanged and keeps `&mut self` for all write
  operations.
- `StructFile<T>::read_value_at` may relax its receiver from `&mut self` to
  `&self`; this is a compatible change for callers and shall be recorded in
  the `StructFile` requirements document when made.
- No serialized format changes.

## Performance and resource use

- Implementations may buffer, cache, or maintain backend cursors to serve
  reads efficiently, behind interior mutability.
- Read operations return owned `T` values. Implementations may need to clone
  values held in memory; the `Vec<T>` implementation therefore requires
  `T: Clone`. Persistent backends may construct owned values while decoding.
- Buffering and cursor updates shall not alter the logical results or ordering
  of reads.
- An iterator returned by `serial_iter(&self)` may own its own buffer
  (as `StructFileIter` does with its `BufferedReader`) rather than share the
  reader's cache, so that iteration and indexed reads do not contend for one
  interior borrow.
- A database backend should prefer chunked iteration (for example keyset
  pagination into a buffer) over holding a live statement cursor inside the
  iterator, since a cursor borrowed from a statement owned by the iterator is
  self-referential.
- `RefCell` and `Cell` add a borrow-flag check per access; `Mutex` and
  `RwLock` add locking cost. No fixed latency, throughput, memory, or
  buffer-capacity guarantee is imposed here. Backend-specific performance
  constraints shall be documented with the corresponding implementation.
- Concurrency and synchronization guarantees are backend-specific and must be
  documented by each implementation (`RSF-RS-011`).

## Phases

### Phase planning and reflection

Plan phases for this project rather than using a fixed, universal sequence.
Before implementation begins, describe how the work should be divided and
consider dependencies, sequencing, risks, and completion criteria. Add, remove,
split, merge, or reorder phases as the project requires. Revisit the plan as
implementation progresses and record meaningful changes or lessons learned.

The number of project-specific phases is decided during this planning
reflection and differs from project to project. Whatever that number is, the
last two phases are always, in this order:

1. **Coverage**: run the tests with coverage, measure coverage for the files
   created or modified by this work only, and update
   [Code coverage](#code-coverage).
2. **Documentation**: update this requirements document (phases, checklist,
   test results, notes, revision) and the code comments (Rust doc comments on
   public items and inline comments) of the touched files.

This document has four project-specific phases, so Coverage is `P-005` and
Documentation is `P-006`.

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Revision 0.1 reader contract:** define `SerialDataReader<T>` with `&mut self` reads, the `Vec<T>` implementation, default searches, and tests T-001 to T-012. | None | Completed in revision 0.1; T-001 to T-012 pass. |
| P-002 | **Trait signature change:** change every read operation of `SerialDataReader<T>` in `src/adapter/abstract_serial_read.rs` to `&self`, update the `Vec<T>` implementation, and update the trait doc comment to describe interior mutability. | P-001 | The trait compiles with no `&mut self` read receiver; `Vec<T>` implements it. |
| P-003 | **Implementor migration:** update `StructFile<T>` (`serial_get`, `serial_iter`, and `read_value_at` to `&self`), move the `BufferedReader` test double's buffer and counters behind `RefCell`/`Cell`, update the `MapReader` double and the test backends in `src/adapter/abstract_data_container.rs`, and relax `find_insert_position` to `&C`. | P-002 | `cargo build --all-targets` and `cargo clippy --all-targets` succeed for `rust_serial_file`; T-001 to T-012 and the existing container and `StructFile` tests pass unchanged in behavior. |
| P-004 | **Shared-reference tests:** add T-013 to T-015 covering reads through `&R`, overlapping iteration and indexed reads on a stateful backend, and `StructFile` reads through a shared reference. | P-003 | T-013 to T-015 are implemented and pass. |
| P-005 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001, P-002, P-003, P-004 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-006 | **Documentation:** update this requirements document and the code comments of the touched files, and record the `read_value_at` receiver change in `doc/requirements/file/struct_file.md`. | P-005 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

Keep this checklist synchronized with the planned phases above. Add one item per
phase and update its state as work progresses. Check the box only when the phase
is complete; use the status label to distinguish phases that are not started,
in progress, or blocked. Record blockers in the item.

Phases are completed in order:

- A phase can be checked only if every phase before it in the checklist is
  already checked. The same rule applies to sub-tasks within a phase.
- When a new project-specific phase is added, it takes the next `P-{XXX}`
  number (after the last project-specific phase), and Coverage and
  Documentation are renumbered to stay last. If Coverage or Documentation
  were already checked, uncheck them and set them back to `Not started`,
  since they must be redone after the new phase. Record the reason in the
  implementation reflection.

- [x] P-001 — `Revision 0.1 reader contract` — Done
- [x] P-002 — `Trait signature change` — Done
- [x] P-003 — `Implementor migration` — Done
- [x] P-004 — `Shared-reference tests` — Done
- [x] P-005 — `Coverage of touched files` — Done
- [x] P-006 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- *Why is this phase breakdown appropriate?* The change is a signature change
  with a fixed blast radius: the trait, its two production implementors, the
  test doubles, and one internal helper. P-002 isolates the contract change;
  P-003 restores a compiling crate; P-004 adds tests for the new guarantee
  (`&self` reads and overlapping reads) that revision 0.1 could not express.
  P-001 records the revision 0.1 work that this document already covered, so
  the existing test IDs keep their history.
- *Are the order and dependencies clear?* Yes. P-002 alone does not compile
  the crate, because implementors still use `&mut self`; P-002 and P-003 are
  therefore expected to land in the same commit, but are kept separate so the
  contract change can be reviewed on its own. P-004 depends on the migrated
  test doubles from P-003.
- *Risks and unknowns:* (1) The `BufferedReader` test double mutates its
  buffer on every read; moving it to `RefCell` must keep borrows scoped per
  call (`RSF-RS-010`), or T-014 will expose a borrow conflict. (2) Downstream
  crates that implement `SerialDataReader<T>` must be migrated; no such
  implementor exists in this workspace today. (3) A future SQLite backend may
  reveal a read path that needs more than `&Connection`; the rusqlite API
  review in [Assumptions and dependencies](#assumptions-and-dependencies)
  suggests it does not.

**Implementation reflection (update as work progresses):**

- Revision 0.2 was planned and implemented on 2026-11-11 on top of
  `e4f5d72`. The plan held; no phase was added, removed, or reordered.
- **P-002:** all read receivers in the trait and the `Vec<T>` implementation
  changed to `&self`. The trait doc comment now states the interior-mutability
  rule, the overlapping-read rule, and that `Send`/`Sync` are not required.
  As planned, the crate did not compile until P-003 was done; both are in the
  same change.
- **P-003:** `StructFile<T>` needed only signature changes, since its reads
  already went through `&FileMetadata`. The `BufferedReader` test double now
  keeps its buffer in a `RefCell<ReadBuffer>` borrowed with `try_borrow_mut`
  for one call, and its read counter in a `Cell`. A `refill_count` counter
  was added so T-014 can prove that refills really happened. `SimulatedBackend`
  in `abstract_data_container.rs` moved its `read_count` to a `Cell`, and
  `find_insert_position` now takes `&C`. The compiler then reported 16
  `mut` bindings in tests that were no longer needed; they were removed with
  `cargo fix`. Callers holding `&mut R` kept compiling, which confirms the
  caller source-compatibility claim.
- **P-004:** T-013 to T-015 were added and pass. Completion criteria met.
- **P-005:** branch coverage could not be measured: `cargo llvm-cov --branch`
  needs a nightly compiler, and only stable is installed. Function, line, and
  region coverage were recorded.
- **P-006:** `Send`/`Sync` doc comments were added to the `Vec<T>` and
  `StructFile<T>` reader implementations (`RSF-RS-011`).
  `doc/requirements/file/struct_file.md` was bumped to revision 0.2 for the
  `read_value_at` receiver change. Only its header, Revisions table, and
  Public API were updated; the rest of that document was not migrated to the
  current template.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `RSF-FR-001`, `RSF-FR-002`, `RSF-FR-003`, `RSF-FR-004`, `RSF-FR-005`, `RSF-FR-006`, `RSF-FR-010`, `RSF-RS-008` | Verify `Vec<T>` provides owned indexed, endpoint, and iteration reads, infallible length/empty queries, and empty-collection behavior. | Reads return expected values in order; invalid indices and empty first/last reads report their typed errors; searching an empty reader returns no match. |
| T-002 | `RSF-FR-011`, `RSF-FR-012`, `RSF-FR-013`, `RSF-RS-006` | Verify value, key, and comparator searches, a key search with context, and both value/key find-and-get operations. | Matching searches return the expected index or value; missing keys return `None`; context is updated to the found index. |
| T-003 | `RSF-RS-007` | Verify key search reports undefined `PartialOrd` ordering. | Searching values containing `NaN` returns the typed `UndefinedOrdering` error rather than panicking. |
| T-004 | `RSF-FR-007`, `RSF-FR-008`, `RSF-FR-009` | Verify reads and iteration across backend buffer state changes. | Refills do not change values, skip records, duplicate records, or reorder iteration. |
| T-005 | `RSF-FR-003`, `RSF-FR-005`, `RSF-FR-011`, `RSF-FR-012` | Verify backend failures during indexed, first, and last reads, iterator creation, and per-item iteration, plus out-of-range errors in the buffered reader. | Each failure remains observable as an error and is not converted into not-found or normal iteration completion. |
| T-006 | `RSF-FR-012` | Verify context-aware searches and find-and-get found, no-match, and error outcomes. | Context-aware methods return the expected result and context; find-and-get returns `Ok(Some(value))`, `Ok(None)`, or `Err` as appropriate. |
| T-007 | `RSF-FR-001`, `RSF-FR-002`, `RSF-FR-003`, `RSF-FR-005`, `RSF-FR-006` | Verify an alternate map-backed reader implements the shared contract through a generic caller, including iterator-creation and out-of-range errors. | Generic read code works without relying on backend-specific method names; reads preserve logical order and failures remain observable. |
| T-008 | `RSF-RS-007`, `RSF-RS-008` | Verify reader error display messages and standard error-source behavior. | All built-in error variants display their documented messages and expose no nested source. |
| T-009 | `RSF-FR-011`, `RSF-FR-012`, `RSF-FR-013`, `RSF-RS-006`, `RSF-RS-007` | Verify empty searches, before-first and after-last results, context seeds on either side and on a match, and inconsistent comparator handling. | Empty and missing values return `None`; context-based searches return the correct result and update the context; an inconsistent comparator returns an error. |
| T-010 | `RSF-FR-005` | Verify collecting an iterator propagates both per-item failures and iterator-creation failures. | Collection returns the original failure instead of silently truncating or succeeding. |
| T-011 | `RSF-FR-003`, `RSF-FR-011` | Verify a search against a sparse map-backed reader propagates a missing-record error. | The missing logical index is reported as an error, not as a search miss. |
| T-012 | `RSF-FR-011`, `RSF-FR-012`, `RSF-RS-007` | Verify search methods propagate backend read failures and undefined partial ordering, including context-aware and find-and-get operations. | Errors remain observable and are not converted into `None` or successful results. |
| T-013 | `RSF-RS-002`, `RSF-RS-005` | Call every read operation (`serial_get`, `serial_first`, `serial_last`, `serial_iter`, and each `serial_find*` form) through a generic function that receives only `&R`, for both `Vec<T>` and the stateful `BufferedReader` double, while a second shared borrow of the same reader is alive. | The code compiles with `&R` only; all results match the expected records and indices. |
| T-014 | `RSF-FR-007`, `RSF-FR-008`, `RSF-FR-009`, `RSF-RS-009`, `RSF-RS-010` | On the `RefCell`-based `BufferedReader` double with a small buffer, interleave `next()` on a live `serial_iter` iterator with `serial_get` and `serial_find` calls that force buffer refills. | The iterator yields every record once and in order; each indexed read and search returns the correct value; no call panics or returns a borrow-conflict error. |
| T-015 | `RSF-RS-002`, `RSF-FR-008`, `RSF-FR-009` | For `StructFile<T>` backed by a temporary file, read through a `&StructFile<T>`: call `serial_get` and `serial_find` while a `serial_iter` iterator is alive, and read the same index repeatedly. | All reads succeed through a shared reference and return the stored records; iteration completes in order; repeated reads return the same value. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `src/adapter/abstract_serial_read.rs` | `vector_reader_returns_owned_records_and_reports_invalid_indices` |
| T-002 | `src/adapter/abstract_serial_read.rs` | `reader_searches_by_value_key_and_comparator` |
| T-003 | `src/adapter/abstract_serial_read.rs` | `undefined_partial_ordering_is_reported` |
| T-004 | `src/adapter/abstract_serial_read.rs` | `buffered_reads_preserve_records_across_refills_and_iteration` |
| T-005 | `src/adapter/abstract_serial_read.rs` | `backend_read_and_iterator_errors_remain_observable` |
| T-006 | `src/adapter/abstract_serial_read.rs` | `context_search_and_find_and_get_report_all_outcomes` |
| T-007 | `src/adapter/abstract_serial_read.rs` | `generic_read_client_supports_an_alternate_backend` |
| T-008 | `src/adapter/abstract_serial_read.rs` | `reader_errors_have_display_messages_and_no_sources` |
| T-009 | `src/adapter/abstract_serial_read.rs` | `searches_cover_empty_boundaries_context_seeds_and_inconsistent_comparators` |
| T-010 | `src/adapter/abstract_serial_read.rs` | `collecting_an_iterator_propagates_item_errors` |
| T-011 | `src/adapter/abstract_serial_read.rs` | `alternate_backend_search_propagates_missing_record_error` |
| T-012 | `src/adapter/abstract_serial_read.rs` | `search_methods_propagate_backend_and_comparison_errors` |
| T-013 | `src/adapter/abstract_serial_read.rs` | `all_read_operations_work_through_a_shared_reference` |
| T-014 | `src/adapter/abstract_serial_read.rs` | `overlapping_iteration_and_indexed_reads_share_interior_state` |
| T-015 | `tests/struct_file.rs` | `struct_file_reads_through_a_shared_reference` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-002 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-003 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-004 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-005 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-006 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-007 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-008 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-009 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-010 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-011 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-012 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-013 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-014 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |
| T-015 | 2026-11-11 | `e4f5d72+dirty` | ✅ Success |

#### Notes

- T-001 to T-012 were first rerun on 2026-11-11 at `e4f5d72` (clean tree)
  as a baseline for the revision 0.1 `&mut self` API: 12 passed, 0 failed.
- After P-004, all tests were run on 2026-11-11 at `e4f5d72+dirty` (the
  revision 0.2 changes not yet committed) with `cargo test` from
  `rust_serial_file/`: 172 passed, 0 failed across all targets. That includes
  14 reader tests (T-001 to T-014) and 25 `StructFile` integration tests
  (including T-015). `cargo fmt --check` and `cargo clippy --all-targets`
  reported no warnings.
- T-001 to T-012 kept their names and assertions. Their only edits were
  removing `mut` bindings that are no longer needed and calling
  `reader.buffer_start()` and `reader.read_count.get()` on the migrated
  `BufferedReader` double.
- T-014 asserts that the double refilled its buffer more often than there
  are records, so the interleaving really moved the buffer while the iterator
  was alive.

#### Warning notes

- None.

### Acceptance criteria

1. The public reader interface and its `serial_`-prefixed read operations are
   documented, and every read operation takes `&self`.
2. Indexed, endpoint, and iteration reads have consistent empty, ordering,
   and out-of-range semantics.
3. Implementations can update internal read state through interior mutability
   without changing the logical results visible to callers, and overlapping
   reads never panic.
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
9. Tests verify the required behavior for a non-stateful in-memory backend, a
   stateful buffered backend using interior mutability, an alternate
   map-backed reader, and the file-backed `StructFile<T>`.
10. `SerialDataContainer<T>` and `SerialDataWriter<T>` behavior is unchanged,
    and existing callers of the reader compile without modification.
11. The crate passes `cargo fmt --check`, `cargo clippy --all-targets`,
    `cargo build`, and `cargo test`.

## Code coverage

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** Measured in P-005 over the full test suite. The table
  lists only the production files touched by revision 0.2. The touched test
  file `tests/struct_file.rs` is test code and is not reported.

### Current coverage results

- **Tool and command:** `cargo-llvm-cov 0.9.1` with `llvm-tools-preview`, on
  the stable toolchain, run from `rust_serial_file/`: `cargo llvm-cov clean`,
  then `cargo llvm-cov --all-targets`, then
  `cargo llvm-cov report --show-missing-lines`. Branch coverage is `N/A`:
  `--branch` needs a nightly compiler.
- **Date of run:** 2026-11-11
- **Git hash:** `e4f5d72+dirty`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `src/adapter/abstract_data_container.rs` | 96.67% (58/60) | 98.09% (513/523) | 97.01% (745/768) | N/A |
| `src/adapter/abstract_serial_read.rs` | 98.59% (70/71) | 98.97% (770/778) | 98.59% (1119/1135) | N/A |
| `src/file/struct_file.rs` | 100.00% (29/29) | 94.47% (205/217) | 82.89% (344/415) | N/A |

### Coverage notes

- **`src/adapter/abstract_data_container.rs`:** Source lines 677-679 and
  726-728 are never executed. They are the two uncovered functions: update
  closures in the test
  `update_stops_after_read_or_closure_failure_and_propagates_write_errors`,
  which checks that the closure is *not* called after a read failure. Not
  executing them is the expected result. The remaining missed lines and
  regions come from generic instantiations that the tests do not drive down
  every path. No further plan.
- **`src/adapter/abstract_serial_read.rs`:** No source line is unexecuted in
  every instantiation. The one uncovered function is the
  `map_err(|_| anyhow!("read buffer is already borrowed"))` closure in the
  `BufferedReader` test double. It is unreachable because the double scopes
  its `RefCell` borrow to one call, which is the behavior `RSF-RS-010`
  requires and T-014 verifies. The 8 missed lines and 16 regions are
  per-instantiation counts of the generic search defaults: each closure type
  passed to a search creates a new instantiation, and an instantiation that
  is not driven down every branch counts as uncovered even when another
  instantiation covers the same source. Adding more closure cases adds more
  instantiations and does not reach 100%. No further plan.
- **`src/file/struct_file.rs`:** Source line 278 (the `Err` arm of
  `u64::try_from(index)` in `StructFileIter::next`) is unreachable on 64-bit
  targets, where every `usize` fits in `u64`. The other missed lines and the
  lower region coverage (82.89%) come from generic `StructFile<T>`
  instantiations: the library build and each test record type instantiate
  the methods separately, and not every instantiation runs every error path.
  The `&self` change added no uncovered code in this file. No further plan.

## Out of scope

- Collection mutation, writing, persistence, and flushing (`SerialDataWriter<T>`
  keeps `&mut self`).
- A specific SQLite, map, or other new backend implementation. The rusqlite
  analysis in this document only establishes that such a backend fits the
  `&self` contract; its design belongs in its own requirements document.
- A particular serialization format or schema.
- A fixed buffering strategy or performance guarantee.
- Making `SerialDataReader<T>` require `Send` or `Sync`; each implementation
  decides (`RSF-RS-011`).
- Ownership of opened files and OS read/write locks across readers; this is
  deferred to the planned `FileAtlas` component. Owner: the user, when that
  component is specified.
- Whether a downstream crate outside this workspace implements
  `SerialDataReader<T>` and needs migration. Owner: the user, before release.
