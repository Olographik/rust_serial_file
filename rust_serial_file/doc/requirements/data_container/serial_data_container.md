# Serial Data Container Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Define a composed read/write interface for serial collections, including sorted insertion and read-modify-write operations.

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

Define `SerialDataContainer<T>` as the combined read/write interface for a
serial collection. It extends `SerialDataReader<T>` and `SerialDataWriter<T>`,
so callers can use the shared read and write operations through one trait.
The collection may be backed by a vector, map, file, SQL database, or another
storage system.

The container adds `serial_auto_insert` from the legacy
`abstract_data_container_old.rs`: it replaces an equal record or inserts a
non-matching record at its sorted position. It also provides operations useful
when a caller needs to combine reading and writing: updating an indexed record
and upserting by a key.

Borrowed mutable access is not part of the backend-neutral container contract.
An optional `SerialDataContainerMut<T>` extension may expose `serial_get_mut`
for backends that can guarantee a stable mutable borrow, such as an in-memory
vector.

All container-specific methods use the `serial_` prefix. Methods inherited
from the reader and writer retain their existing contracts.

## Goals and scope

### Goals

- Define a public `SerialDataContainer<T>` trait extending both
  `SerialDataReader<T>` and `SerialDataWriter<T>`.
- Add sorted auto-insertion, index-based read-modify-write, and key-based
  upsert operations with clear ordering and failure semantics.
- Keep the combined interface usable by persistent backends that return owned
  records rather than references into storage.
- Specify optional borrowed mutable access separately from the core contract.

### Scope

- Specify the combined trait and its additional operations.
- Reuse reader and writer requirements for all inherited operations rather
  than redefining their semantics.
- Specify a `Vec<T>` implementation as the initial reference implementation.
- Keep file, SQL, and other persistent backend implementations as future
  implementations of the same contract.
- Preserve the behavior of the reader and writer traits.

## Assumptions and dependencies

- `SerialDataReader<T>` and `SerialDataWriter<T>` are public traits available
  from the adapter module.
- Reader operations return owned `T` values and writer operations accept
  owned `T` values; the container does not require `T: Clone` globally.
- Reader and writer operations use `anyhow::Result` for observable backend
  failures.
- `SearchContext` is the established type for context-aware reader searches.
- The exact Rust edition and MSRV follow the crate-wide decisions.
- Search-based insertion requires records to be consistently ordered under
  the comparison used for that operation.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| RSF-FR-001 | The interface shall expose a public `SerialDataContainer<T>` trait that extends `SerialDataReader<T>` and `SerialDataWriter<T>`. |
| RSF-FR-002 | Every new method introduced by this interface shall use the `serial_` prefix. Inherited methods shall retain their existing names and contracts. |
| RSF-FR-003 | `serial_auto_insert(value)` shall use the natural `T: PartialOrd<T>` ordering. If an equal record exists, it shall replace that record; otherwise, it shall insert before the first greater record, or append when no greater record exists. |
| RSF-FR-004 | `serial_auto_insert` shall return `anyhow::Result<()>`, report comparison and backend failures, and not report success when the required read or write fails. |
| RSF-FR-005 | `serial_update(index, update)` shall read the owned record at `index`, pass it to a fallible update closure, and write the returned replacement to the same index. It shall return `anyhow::Result<()>`. |
| RSF-FR-006 | `serial_update` shall not invoke the update closure when the initial read fails. If the closure fails, it shall not write a replacement. Backend write failure behavior shall follow the writer's documented partial-failure guarantees. |
| RSF-FR-007 | `serial_upsert(key, value)` shall use `T: PartialOrd<K>` to find the key's sorted position. It shall replace a record whose comparison is `Equal`, insert before the first record comparing `Greater`, or append if all records compare `Less`. It shall return the index that was inserted or replaced. |
| RSF-FR-008 | `serial_upsert` shall report read, comparison, and write failures as errors. The supplied value shall be consistent with the key ordering so the collection remains ordered after insertion or replacement. |
| RSF-FR-009 | Read-modify-write operations shall preserve logical length and order except for the explicitly described insertion behavior of `serial_auto_insert` and `serial_upsert`. |
| RSF-FR-010 | The core `SerialDataContainer<T>` contract shall not require returning a reference into backend storage. A separate optional `SerialDataContainerMut<T>` extension may provide `serial_get_mut(index) -> anyhow::Result<&mut T>` for backends that can safely support it. |
| RSF-FR-011 | `serial_get_mut` shall return a distinguishable out-of-range error and shall not be required for file, SQL, or other backends that cannot provide a stable mutable borrow. |
| RSF-FR-012 | Implementations shall maintain consistency between the reader's logical contents and the writer's mutations; successful writes shall be visible to subsequent reads on the same container. |

### Data and serialization

| ID | Requirement |
|---|---|
| RSF-DS-001 | The container shall be generic over `T` and shall not prescribe a serialization format or require a serialization trait. |
| RSF-DS-002 | Backends shall preserve their own encoding, record-boundary, and compatibility rules when reading or writing container records. |
| RSF-DS-003 | An update or upsert shall not silently substitute a different value or report success for a partial or invalid write. |
| RSF-DS-004 | After a successful `serial_auto_insert` or `serial_upsert`, the collection shall be ordered according to the comparison used by that operation. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| RSF-RS-001 | The core trait shall extend `SerialDataReader<T> + SerialDataWriter<T>` without adding unnecessary bounds to `T`. |
| RSF-RS-002 | `serial_update` shall accept a one-shot fallible closure that consumes an owned record and returns an owned replacement, avoiding a requirement for `T: Clone` or a borrowed backend value. |
| RSF-RS-003 | `serial_auto_insert` shall require `T: PartialOrd<T>` only for that operation. |
| RSF-RS-004 | `serial_upsert<K>` shall require `T: PartialOrd<K>` only for that operation. |
| RSF-RS-005 | The optional mutable-access extension shall express the borrow lifetime in its return type and shall only be implemented where the backend can uphold Rust's exclusive-borrow guarantees. |
| RSF-RS-006 | The traits shall use the crate's established Rust edition and MSRV; this document does not set a separate edition or MSRV. |

## Public API

The combined trait shall compose the existing reader and writer APIs:

```rust
pub trait SerialDataContainer<T>:
    SerialDataReader<T> + SerialDataWriter<T>
{
    fn serial_auto_insert(&mut self, value: T) -> anyhow::Result<()>
    where
        T: PartialOrd<T>;

    fn serial_update<F>(&mut self, index: usize, update: F) -> anyhow::Result<()>
    where
        F: FnOnce(T) -> anyhow::Result<T>;

    fn serial_upsert<K>(&mut self, key: &K, value: T) -> anyhow::Result<usize>
    where
        T: PartialOrd<K>;
}
```

Optional mutable access is provided separately:

```rust
pub trait SerialDataContainerMut<T>: SerialDataContainer<T> {
    fn serial_get_mut(&mut self, index: usize) -> anyhow::Result<&mut T>;
}
```

- `serial_auto_insert` follows natural `T` ordering; equal values are
  replacements, not duplicates.
- `serial_update` reads one owned value, applies the closure, then writes its
  replacement at the original index. A successful update does not change the
  collection length.
- `serial_upsert` compares records with the supplied key, replaces an equal
  record, or inserts at the key's sorted position. Its returned index identifies
  the replaced or newly inserted record.
- `serial_get_mut` is not a requirement for implementations of the core
  `SerialDataContainer<T>`.

The exact implementation of binary search and insertion-position discovery
must preserve the comparison semantics of `SerialDataReader<T>`. In
particular, `serial_upsert` must identify the first position at which the key
is not greater than the record and must propagate undefined comparisons.

## Errors and failure behavior

- Reader and writer failures shall retain the errors and semantics established
  by their respective trait contracts.
- `serial_auto_insert` and `serial_upsert` shall surface errors encountered
  while reading, comparing, inserting, or replacing.
- `serial_update` shall surface the indexed read error, closure error, or
  writer error. It shall not call the closure after a read failure or write
  after a closure error.
- An invalid index passed to `serial_update` shall be reported by the reader
  operation; no replacement shall be written.
- Implementations shall document whether a backend failure can occur after
  partial persistent mutation. Errors shall never be converted into
  success-shaped results.
- `serial_get_mut` shall return an error for an invalid index and shall not
  silently return a fabricated mutable value.

## Safety and security

- Safe Rust is sufficient for the required API; `unsafe` is not required.
- An implementation of `SerialDataContainerMut<T>` must uphold exclusive
  mutable-borrow guarantees and prevent invalidation of the returned reference
  while it is live.
- Backends shall validate indexes and storage-derived record boundaries before
  accessing or modifying data.
- Errors and logs shall not expose secrets or sensitive record contents.

## Compatibility

- `SerialDataReader<T>` and `SerialDataWriter<T>` remain independently usable;
  the combined trait composes them without changing their APIs.
- The legacy `SerialDataContainer<T>` in
  `src/adapter/abstract_data_container_old.rs` is a source for the
  `serial_auto_insert` behavior, not a requirement to preserve its infallible
  method signatures.
- Legacy names such as `auto_insert`, `write_at`, and `serial_push` shall not
  be introduced by the new contract; new methods use the `serial_` convention.
- `serial_get_mut` is an optional capability and shall not constrain
  implementations for persistent backends.

## Performance and resource use

- Search-based insertion should use logarithmic comparisons when the backend
  can provide indexed reads efficiently. Storage-specific I/O costs may differ.
- `serial_update` may require a read followed by a write; the interface does
  not guarantee atomicity or transactionality.
- `serial_get_mut` may be more efficient for in-memory collections but is not
  available to every backend.
- No fixed latency, throughput, memory, or durability guarantee is imposed.
  Implementations shall document backend-specific guarantees.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Finalize the composed trait API, `serial_auto_insert`, `serial_update`, key-based `serial_upsert`, and optional mutable-access extension. | None. | Signatures, ordering, replacement, and error semantics are unambiguous and consistent with reader/writer contracts. |
| P-002 | Add contract and simulated-backend tests for composition, sorted insertion/upsert, update outcomes, ordering failures, and backend failures. | P-001. | Every new behavior and relevant failure case has a test mapped to these requirements. |
| P-003 | Implement the combined and optional mutable-access traits for `Vec<T>`, preserving non-`Clone` support where possible. | P-001; P-002. | Vector implementation compiles and all successful operations preserve specified values, order, and indexes. |
| P-004 | Run focused and crate-level validation, then update test statuses and coverage results. | P-002; P-003. | Tests, formatting, Clippy, and build checks pass; measured coverage scope and results are recorded. |

### Current implementation checklist

- [ ] P-001 — `Finalize composed container API` — Not started
- [ ] P-002 — `Implement container contract tests` — Not started
- [ ] P-003 — `Implement container for Vec<T>` — Not started
- [ ] P-004 — `Validate container implementation` — Not started

**Planning reflection:**

- Composition keeps the established read and write contracts as independent,
  reusable traits. Container-specific behavior is limited to operations that
  combine reads and writes.
- Tests precede the vector implementation so behavior, especially sorted
  replacement versus insertion, is established independently of one backend.
- Borrowed mutable access is separated because it cannot be guaranteed by
  file and SQL backends that return owned records.
- `serial_update` and key-based `serial_upsert` need particular review:
  backend failures can interrupt multi-step operations, and upsert must
  preserve ordering under the selected comparison.

**Implementation reflection:** Not started.

## Testing and acceptance criteria

### Tests

The following are planned tests. Their status remains **⏳ Not run** until
implemented and executed.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | RSF-FR-001, RSF-RS-001 | Verify a generic caller can use both reader and writer methods through `SerialDataContainer<T>`. | The combined trait exposes inherited operations and preserves their established behavior. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `container_composes_reader_and_writer` | ⏳ Not run |
| T-002 | RSF-FR-003, RSF-FR-004, RSF-DS-004 | Auto-insert values into an empty, sorted, and unsorted-position vector, including a duplicate. | New values are inserted in order; an equal value replaces the existing record. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `auto_insert_orders_and_replaces_records` | ⏳ Not run |
| T-003 | RSF-FR-004 | Exercise auto-insert comparison and backend failures. | Each failure is returned and no success is reported. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `auto_insert_propagates_read_and_write_errors` | ⏳ Not run |
| T-004 | RSF-FR-005, RSF-FR-006 | Update an indexed record successfully and exercise read, closure, and write failures. | Successful updates replace only the requested value; read and closure failures do not proceed to a write. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `update_reads_transforms_and_writes_record` | ⏳ Not run |
| T-005 | RSF-FR-007, RSF-FR-008, RSF-FR-009 | Upsert a key that is before, equal to, between, and after existing records. | The returned index is correct; matches are replaced and misses inserted in key order. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `upsert_inserts_or_replaces_by_key` | ⏳ Not run |
| T-006 | RSF-FR-008 | Exercise undefined comparison and simulated read/write errors during upsert. | Errors remain observable and do not produce a successful result. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `upsert_propagates_comparison_and_backend_errors` | ⏳ Not run |
| T-007 | RSF-FR-010, RSF-FR-011, RSF-RS-005 | Verify optional mutable access on a vector and reject an out-of-range index. | A valid borrow mutates the selected value; invalid access returns an error. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `mutable_extension_returns_checked_vector_access` | ⏳ Not run |
| T-008 | RSF-RS-002, RSF-RS-003, RSF-RS-004 | Verify update and insertion operations work without imposing `Clone` on the record type. | A non-`Clone` record type compiles and supports operations not requiring cloning. | `rust_serial_file/src/adapter/abstract_serial_data_container.rs` | `container_operations_support_non_clone_records` | ⏳ Not run |

### Acceptance criteria

1. `SerialDataContainer<T>` publicly extends both reader and writer traits.
2. `serial_auto_insert`, `serial_update`, and `serial_upsert` have
   unambiguous ordering, replacement, return, and failure semantics.
3. Optional `serial_get_mut` does not constrain the backend-neutral core
   trait or require persistent backends to return borrowed records.
4. Tests cover success, boundaries, comparison failures, and simulated
   backend failures.
5. Existing reader and writer behavior remains unchanged.
6. Applicable tests, formatting, Clippy, and build checks pass.

## Code coverage

Coverage has not yet been measured because implementation and tests have not
started.

- **Coverage tool:** Not run.
- **Measured scope:** Not measured.
- **Coverage summary:** To be recorded after implementation.

## Out of scope

- File, SQL, or other persistent backend implementations.
- A serialization format or schema.
- Atomic transactions or rollback guarantees beyond those documented by
  individual backends.
- Requiring borrowed mutable access from all `SerialDataContainer<T>`
  implementations.
- Concurrent mutation and synchronization policy.
