# Serial Write Container Requirements

- **Date:** 2026-11-07
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Define a shared write interface for serial collections backed by in-memory containers, files, databases, and other storage.

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

Define `SerialDataWriter<T>`, the write portion of a serial collection
interface. A serial collection may be backed by an in-memory vector or map, a
file, a SQL database, or another storage system. Callers use the same
collection-oriented write operations without depending on the storage backend.

Every mutating operation introduced by the interface uses the `serial_` prefix
to avoid name conflicts with backend methods. The existing
`src/adapter/abstract_data_container.rs` draft is a reference for collection
operations and semantics; this document specifies a backend-neutral, fallible
writer contract.

## Goals and scope

### Goals

- Define a backend-neutral write interface for serial collections of `T`.
- Provide a primary operation for appending a record and operations for common
  indexed and whole-collection mutations.
- Make persistence, storage, and validation failures observable to callers.
- Define consistent logical indexing and mutation behavior across backends.

### Scope

- Specify the public `SerialDataWriter<T>` trait and its write operations.
- Cover in-memory, file-backed, map-like, and database-backed implementations
  as possible backends.
- Define behavior for append, insert, replacement, removal, clearing, and
  truncation of the logical collection.
- Exclude read and search operations; those belong to a reader interface.
- Keep backend-specific encoding, storage layout, and transaction policy out of
  the shared contract unless they affect the observable write result.

## Assumptions and dependencies

- The writer exposes a logical ordered sequence of records addressable by
  zero-based indices. A backend that is not naturally sequential must define
  how its storage maps to that logical order.
- The caller and implementation agree on the mapping between logical records
  and backend storage.
- Write operations may update memory, buffers, files, databases, or other
  storage, and may fail while doing so.
- The interface uses `anyhow::Result` so it can report failures from different
  storage backends through one API. The crate must depend on `anyhow`.
- The exact Rust edition and MSRV are not established by this requirements
  document. These must follow the crate's project-wide decisions.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| RSF-FR-001 | The interface shall expose a public `SerialDataWriter<T>` trait representing write operations for a serial collection. |
| RSF-FR-002 | Every mutating operation introduced by the interface shall have a `serial_`-prefixed name to avoid collisions with backend methods. |
| RSF-FR-003 | The interface shall provide `serial_write(&mut self, value: T) -> anyhow::Result<()>` to append one record to the end of the logical collection. |
| RSF-FR-004 | The interface shall provide `serial_insert(index, value)` to insert a record at a zero-based logical position. Inserting at the current length appends; an index greater than the length shall fail without changing the collection. |
| RSF-FR-005 | The interface shall provide `serial_write_at(index, value)` to replace the record at an existing zero-based logical index. An index outside the collection shall fail without changing the collection. |
| RSF-FR-006 | The interface shall provide `serial_remove(index)` to remove the record at an existing zero-based logical index and return `anyhow::Result<()>`. An index outside the collection shall fail without changing the collection. |
| RSF-FR-012 | The interface shall provide `serial_remove_get(index)` to remove and return the record at an existing zero-based logical index as `anyhow::Result<T>`. An index outside the collection shall fail without changing the collection. |
| RSF-FR-007 | The interface shall provide `serial_clear()` to remove all records. Clearing an already empty collection shall succeed. |
| RSF-FR-008 | The interface shall provide `serial_truncate(len)` to retain at most the first `len` records and discard any records after them. A requested length greater than or equal to the current length shall leave the collection unchanged and succeed. |
| RSF-FR-009 | A successful operation shall make its specified logical mutation observable to subsequent operations, subject to the backend's documented persistence guarantees. |
| RSF-FR-010 | A failed operation shall return an error and shall not report success. The implementation shall document whether a backend failure can leave a partial mutation; where possible, a failed operation shall leave the logical collection unchanged. |
| RSF-FR-011 | Implementations shall maintain a consistent logical record order and index mapping after each successful insertion, removal, replacement, clear, or truncate operation. |

### Data and serialization

| ID | Requirement |
|---|---|
| RSF-DS-001 | The interface shall be generic over record type `T` and shall not prescribe a serialized representation or require `T` to implement a particular serialization trait. |
| RSF-DS-002 | A backend that encodes or maps `T` to stored data shall preserve its own format, record-boundary rules, and documented compatibility guarantees. |
| RSF-DS-003 | A successful write shall store or apply the supplied record value as defined by the backend, without silently substituting another value. |
| RSF-DS-004 | The backend shall not present a partially encoded or invalid record as a successfully written record. Any malformed or incomplete storage state and its recovery behavior shall be documented, and failures shall be observable. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| RSF-RS-001 | Write operations shall take ownership of inserted or replacement values. `serial_remove_get` shall return ownership of the removed value; `serial_remove` shall return `Ok(())` on success. |
| RSF-RS-002 | Mutating operations shall take `&mut self` to make exclusive access to writer state explicit. |
| RSF-RS-003 | Each fallible operation shall return `anyhow::Result`; `serial_remove` shall return `Ok(())` on success, while `serial_remove_get` shall return the removed `T` in `Ok(T)`. |
| RSF-RS-004 | The trait shall not impose unnecessary trait bounds on `T`; bounds shall be limited to operations that actually need them. |
| RSF-RS-005 | An implementation for `Vec<T>` shall be possible without requiring `T: Clone`. |
| RSF-RS-006 | The trait shall use the crate's established Rust edition and MSRV; this document does not set a separate edition or MSRV. |

## Public API

The required core API is:

```rust
pub trait SerialDataWriter<T> {
    fn serial_write(&mut self, value: T) -> anyhow::Result<()>;
    fn serial_insert(&mut self, index: usize, value: T) -> anyhow::Result<()>;
    fn serial_write_at(&mut self, index: usize, value: T) -> anyhow::Result<()>;
    fn serial_remove(&mut self, index: usize) -> anyhow::Result<()>;
    fn serial_remove_get(&mut self, index: usize) -> anyhow::Result<T>;
    fn serial_clear(&mut self) -> anyhow::Result<()>;
    fn serial_truncate(&mut self, len: usize) -> anyhow::Result<()>;
}
```

- `serial_write` appends one record.
- `serial_insert` inserts before the record currently at `index`; `index ==
  len` appends.
- `serial_write_at` replaces an existing record without changing the logical
  collection length.
- `serial_remove` removes the selected record without returning its value.
- `serial_remove_get` removes the selected record and returns ownership of it.
- `serial_clear` removes all records.
- `serial_truncate` retains the prefix up to the requested length.
- All methods are public trait methods and fallible so storage and persistence
  errors can be reported.
- The trait does not require `Index`, iteration, length reporting, search, or
  read methods. These belong to a reader or combined collection interface.

## Errors and failure behavior

- Backend write, encoding, decoding needed for replacement/removal, validation,
  and persistence failures shall be returned as `Err`.
- Invalid indices for insertion, replacement, and removal shall be reported as
  errors. Insertion at exactly the logical length is valid.
- Errors shall not be silently ignored or converted into success-shaped
  results.
- Implementations shall document transactional guarantees and whether an error
  can occur after some backend state has already changed. They shall preserve
  logical consistency if rollback is unavailable.
- Clearing an empty collection and truncating to a length not smaller than the
  current length shall be successful no-ops.

## Safety and security

- Safe Rust shall be used; `unsafe` code is not required by this interface. Any
  implementation that uses `unsafe` must document its safety invariants and
  receive the project's normal unsafe-code review.
- Backends shall validate indices, record lengths, and storage-specific input
  before using them to access or modify storage.
- Implementations shall not expose secrets or sensitive record contents in
  errors or logs.
- File and database implementations shall follow the project's established
  rules for permissions, transaction handling, and safe resource cleanup.

## Compatibility

- All writer operations shall use `serial_`-prefixed method names, including
  replacement (`serial_write_at`) to avoid collisions with backend methods.
- Logical index semantics shall be consistent across implementations.
- The draft in `src/adapter/abstract_data_container.rs` is design input, not a
  public compatibility commitment. Its infallible operations and unprefixed
  `write_at` method may be replaced by the fallible API specified here.
- This interface shall not change the behavior of existing reader APIs.
- Backend data-format compatibility remains the responsibility of each
  implementation.

## Performance and resource use

- Implementations shall avoid unnecessary full-collection rewrites when a
  backend can perform the requested mutation more efficiently.
- The shared interface does not guarantee operation complexity, atomicity,
  buffering strategy, or durability beyond the documented backend behavior.
- Implementations shall document relevant resource use, buffering, and
  concurrency constraints.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Finalize the trait API and error semantics. Confirm signatures, index behavior, and how partial failures are represented. | None. | API decisions are recorded, and each required operation has unambiguous success and failure semantics. |
| P-002 | Implement contract tests for the writer API, including success, boundaries, both removal forms, and simulated backend failures. | P-001. | Tests cover every required operation and are mapped to the relevant requirement IDs; failure behavior is exercised with a simulated backend. |
| P-003 | Implement `SerialDataWriter<T>` for `Vec<T>` without unnecessary bounds on `T`. | P-001; P-002. | The trait implementation compiles for a non-`Clone` record type and preserves the specified collection semantics. |
| P-004 | Run and validate the contract tests against the `Vec<T>` implementation and simulated backend; resolve any failures and verify applicable formatting, lint, and build checks. | P-002; P-003. | All applicable tests and checks pass, and their statuses are recorded in the test table. |

### Current implementation checklist

- [x] P-001 — `Finalize writer API and error semantics` — Complete
- [x] P-002 — `Implement writer contract tests` — Complete
- [x] P-003 — `Implement writer for Vec<T>` — Complete
- [ ] P-004 — `Validate tests and implementation` — In progress; tests and formatting pass, but Clippy is blocked by existing diagnostics in unrelated files.

**Planning reflection (complete before implementation):**

- Why is this phase breakdown appropriate for the project?
  - The API and failure semantics should be settled before implementation.
    `Vec<T>` provides a small reference implementation, while a simulated
    backend exercises error behavior without expanding this work into
    storage-specific integrations.
- Are the phase order and dependencies clear, and can phases be implemented and
  verified independently where practical?
  - Yes. The API is settled first, then contract tests are written before the
    reference `Vec<T>` implementation. Both the implementation and simulated
    failure behavior are validated in the final phase.
- What risks, unknowns, or decisions could change the phase plan?
  - The main open risk is how partial failures should be documented. The
    simulated backend should exercise the agreed failure contract without
    implying guarantees for any particular storage backend.

**Implementation reflection (update as work progresses):**

- The plan remains appropriate. The trait contract, contract tests, and
  `Vec<T>` implementation are complete; the simulated writer verifies that
  backend errors remain observable.
- P-004's tests and formatting criteria passed. The full crate test suite
  passed, but Clippy with warnings denied reported existing diagnostics in
  unrelated files, so the phase remains in progress until those checks can be
  validated without changing unrelated code.
- The focused writer tests and full crate suite passed; formatting passed.
  Coverage has not been measured.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `RSF-FR-001`, `RSF-FR-002`, `RSF-RS-004`, `RSF-RS-005` | Verify a generic `SerialDataWriter<T>` caller can use the trait with a `Vec<T>` whose element type is not `Clone`. | Trait calls compile and mutate the vector as specified without a `Clone` bound. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `writer_works_with_non_clone_records` | ✅ Success |
| T-002 | `RSF-FR-003`, `RSF-FR-009` | Append a record to an empty and a non-empty collection. | Each call returns `Ok(())` and the value appears at the end in logical order. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_write_appends_to_empty_and_non_empty_vectors` | ✅ Success |
| T-003 | `RSF-FR-004`, `RSF-FR-011` | Insert at the start, middle, and end; attempt an index greater than the length. | Valid inserts produce the expected order; the invalid index returns `Err` and does not change the collection. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_insert_handles_boundaries_and_rejects_out_of_range_indices` | ✅ Success |
| T-004 | `RSF-FR-005`, `RSF-FR-011` | Replace existing records and attempt replacement at an out-of-range index. | A valid replacement changes only the target record; an invalid index returns `Err` and leaves the collection unchanged. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_write_at_replaces_existing_record_and_rejects_invalid_index` | ✅ Success |
| T-005 | `RSF-FR-006`, `RSF-RS-001`, `RSF-RS-003` | Remove the first, middle, and last record with `serial_remove`; attempt removal from an invalid index. | Valid calls return `Ok(())` and remove the selected record; invalid removal returns `Err` without mutation. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_remove_removes_without_returning_value` | ✅ Success |
| T-008 | `RSF-FR-012`, `RSF-RS-001`, `RSF-RS-003` | Remove the first, middle, and last record with `serial_remove_get`; attempt removal from an invalid index. | Valid calls return the removed value and preserve remaining order; invalid removal returns `Err` without mutation. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_remove_get_returns_removed_value` | ✅ Success |
| T-006 | `RSF-FR-007`, `RSF-FR-008` | Clear empty and non-empty collections; truncate to zero, a smaller length, and a length at least as large as the current length. | Collection contents match the specified semantics and no-op cases succeed. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `serial_clear_and_truncate_follow_collection_semantics` | ✅ Success |
| T-007 | `RSF-FR-010`, `RSF-DS-004` | Use a simulated backend to produce a write or persistence failure. | The method returns `Err`; the test verifies the simulated backend's documented partial-failure or rollback behavior. | `rust_serial_file/src/adapter/abstract_serial_write.rs` | `backend_write_failures_are_returned_to_the_caller` | ✅ Success |

### Acceptance criteria

1. The specified public trait and all required `serial_`-prefixed operations
   are implemented and documented.
2. Append, indexed mutation, removal, clearing, and truncation follow their
   specified logical semantics.
3. Backend failures and invalid indices remain observable; partial-failure
   behavior is documented and tested.
4. Implementations preserve record order and consistency after successful
   operations.
5. Relevant formatting, lint, build, and test checks pass without changing
   existing reader behavior.

## Code coverage

Coverage has not yet been measured because implementation has not started.
Record the coverage tool, measured scope, percentage, threshold, and any
uncovered areas here when tests exist.

## Out of scope

- Reading, iteration, indexing traits, and search operations.
- A common serialization format or serialization trait bound for `T`.
- Backend-specific transaction, durability, and concurrency guarantees beyond
  documenting their effect on the write result.
- Sorted insertion and search-based insertion such as the draft's
  `auto_insert`; these require ordering semantics and may be specified
  separately.
- A universal collection length API; length reporting belongs to the reader
  or a combined collection contract.
