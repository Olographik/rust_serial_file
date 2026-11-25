# StructFile Requirements

- **Date:** 2026-11-11
- **Author:** `Olographik`
- **Revision:** 0.4
- **Short description:** Specify a typed, file-backed serial collection that is generic over how it holds its `FileMetadata` (owned or borrowed), with read-only and writable views.

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

Define `StructFile<T, M>`, a typed serial collection over fixed-width rows
stored through a `FileMetadata`. The type uses `T: SerializedStruct` to encode
values into and decode values from rows. It implements the serial collection
traits so clients can read, iterate, append, insert, replace, remove, clear,
truncate, update, and search file-backed records with the same API as other
serial collections.

Revisions 0.1 and 0.2 required `StructFile<T>` to own its `FileMetadata`.
That prevents using it with `FileLock`, which also owns the metadata, and with
the planned `FileAtlas`, which will own all opened files and hand out typed
views of them. Revision 0.3 makes the collection generic over its storage
`M`: any type that implements `Borrow<FileMetadata>`. The same code then
covers:

- an owned `FileMetadata` (the default, unchanged from revision 0.2);
- a borrowed `&FileMetadata`, giving a **read-only view**
  (`StructReadOnlyFile<'a, T>`);
- a borrowed `&mut FileMetadata`, giving a **writable view**
  (`StructWriteFile<'a, T>`);
- any other `Borrow`/`BorrowMut` holder chosen by the caller, such as `Box`
  or `Arc`;
- a `FileLock` guard (`FileReadGuard`, `FileWriteGuard`) in a multi-threaded
  application, held directly by the view since `FileLock` revision 0.5, or
  a reborrow of it (`&*guard`, `&mut *guard`).

`StructFile` does not know about `FileLock`, its guards, or any locking. A
single-threaded application uses owned or borrowed metadata and needs no
lock. A view held through a borrow keeps its cached length correct, because
the borrow prevents any other in-process access from changing the file while
the view exists.

The file metadata prefix remains opaque to `StructFile`; the component only
interprets the row region. The caller chooses and manages metadata bytes when
constructing the `FileMetadata`.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-11 | `Olographik` | Public API: `read_value_at`, `serial_get`, and `serial_iter` take `&self` instead of `&mut self`, following revision 0.2 of `doc/requirements/data_container/serial_read_container.md`. No requirement IDs changed. The rest of this document still follows the previous template. |
| 0.3 | 2026-11-11 | `Olographik` | `StructFile` becomes generic over its storage `M` instead of owning a `FileMetadata`. Modified: `RSF-SF-FR-001` (generic storage), `RSF-SF-FR-003` (path constructor only for owned storage), `RSF-SF-FR-005` (reader requires `M: Borrow<FileMetadata>`), `RSF-SF-FR-006` and `RSF-SF-FR-007` (writer and container require `M: BorrowMut<FileMetadata>`), `RSF-SF-FR-011` (`into_inner` recovers the storage), `RSF-SF-RS-001` (generic type, no `Clone`), `RSF-SF-RS-002` (iterator borrows the view). Added: `RSF-SF-FR-014` (read-only view alias), `RSF-SF-FR-015` (writable view alias), `RSF-SF-FR-016` (`as_read_only`), `RSF-SF-DS-010` (length recomputed per view), `RSF-SF-RS-007` (storage bounds, no lock dependency), `RSF-SF-RS-008` (default storage parameter), `RSF-SF-RS-009` (`Send`/`Sync` follow `T` and `M`), `RSF-SF-RS-010` (consistent `borrow`). Document migrated to the current requirements template: Revision column, closing Coverage and Documentation phases, split test tables, and the coverage results table. Tests T-026 to T-031 added. |
| 0.4 | 2026-11-11 | `Olographik` | Modified: `RSF-SF-RS-007` (lock guards can be held directly as storage, not only reborrowed). No behavior or code changed. `FileLock` revision 0.5 added `Borrow`/`BorrowMut<FileMetadata>` impls to its guards, so a view can now hold a guard directly. Updated Overview, Assumptions, the lock usage example, Safety, Compatibility, and Out of scope (the guard `Borrow` item is resolved). The `StructFile` type doc comment in `src/file/struct_file.rs` was reworded to match; no code changed. |

## Goals and scope

### Goals

- Bind a `FileMetadata` row width to `T::SIZE` and provide typed fallible
  row reads and writes using `SerializedStruct`.
- Let the caller decide whether the collection owns its `FileMetadata` or
  borrows it, without any dependency on locking.
- Offer a read-only view type that cannot write, and a writable view type,
  both enforced at compile time.
- Implement `SerialDataReader<T>` for every view, and `SerialDataWriter<T>`
  and `SerialDataContainer<T>` for writable views.
- Preserve serial collection semantics, including ordered insertion,
  replacement, removal, and truncation.
- Keep file I/O, serialization, and index-conversion failures observable.
- Keep existing code that uses `StructFile<T>` with owned metadata
  compiling.

### Scope

- Specify the `StructFile<T, M>` type, its storage bounds, constructors,
  access to the metadata, typed row operations, and the view aliases.
- Specify the collection trait implementations and the iterator.
- Specify the behavior of collection mutations that shift file-backed rows.
- Preserve `FileMetadata` layout and behavior, the existing serial collection
  traits, and the owned-storage API of revision 0.2.
- Exclude `FileLock` and `FileAtlas` changes. They may use `StructFile`, but
  `StructFile` does not depend on them.

## Assumptions and dependencies

- The component uses the existing `FileMetadata` type and its row methods:
  `element_size`, `element_count`, `read_row`, `read_rows`, `write_row`,
  `write_rows`, `append_row`, `insert_row`, and `truncate_rows`. Reads take
  `&FileMetadata`; writes take `&mut FileMetadata`.
- `T` implements `SerializedStruct`; `T::SIZE` is the fixed serialized row
  width. Implementations of `SerializedStruct` are responsible for providing
  compatible `EncodeRaw` and `DecodeRaw` behavior.
- `FileMetadata` remains the sole owner of the file handle. `StructFile`
  holds an `M` that gives access to one `FileMetadata`; it never clones the
  metadata or the file handle.
- The row count is read from the metadata when a view is constructed and is
  maintained by successful operations on that view. While a view exists, the
  borrow or ownership of `M` prevents other in-process code from changing the
  same `FileMetadata` through safe Rust. Changes through another file handle
  or another process are unsupported; coordinating them is the caller's job,
  and later the planned `FileAtlas`'s (OS file locks).
- `M`'s `borrow` and `borrow_mut` implementations return the same
  `FileMetadata` on every call while the view exists. This holds for owned
  values, references, `Box`, `Arc`, and the `FileLock` guards, whose
  `Borrow` impls delegate to `Deref` (`FileLock` requirement `FL-RS-008`).
- The component implements the existing `SerialDataReader<T>`,
  `SerialDataWriter<T>`, and `SerialDataContainer<T>` traits from the adapter
  module, with reads on `&self` as required by revision 0.2 of
  `doc/requirements/data_container/serial_read_container.md`. Collection
  searches and `serial_auto_insert` require records to be sorted according to
  the applicable comparison, as defined by those traits.
- `write_multiple` receives records in the order the caller wants stored.
  The caller is responsible for ordering them; `StructFile` neither sorts nor
  validates their ordering.
- Dependencies: `anyhow` 1.0, the workspace `serialized_struct_derive` and
  `utils` crates, and `tempfile` for tests. Rust edition 2024; no MSRV is set
  by this document.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `RSF-SF-FR-001` | The component shall provide a public `StructFile<T, M = FileMetadata>` that requires `T: SerializedStruct` and holds one storage value `M` giving access to a `FileMetadata`. The storage may be owned (`FileMetadata`) or borrowed (`&FileMetadata`, `&mut FileMetadata`, or another `Borrow<FileMetadata>` holder such as `Box` or `Arc`). | 0.3 |
| `RSF-SF-FR-002` | Construction from `FileMetadata` shall validate that `T::SIZE` is nonzero and equals the metadata row width. | 0.1 |
| `RSF-SF-FR-003` | The component shall provide a path-based constructor, available only for owned storage (`M = FileMetadata`), that opens or creates a file through `FileMetadata`, using `T::SIZE` as its row width and accepting a metadata-prefix offset. | 0.3 |
| `RSF-SF-FR-004` | The component shall provide typed read and replacement operations for existing rows, encoding and decoding each row through `SerializedStruct`. | 0.1 |
| `RSF-SF-FR-005` | For any `M: Borrow<FileMetadata>`, the component shall implement `SerialDataReader<T>`, returning owned decoded values and an iterator of fallible owned values. | 0.3 |
| `RSF-SF-FR-006` | For any `M: BorrowMut<FileMetadata>`, the component shall implement `SerialDataWriter<T>` with append, insert, replacement, removal, removal-and-return, clear, and truncate operations using serial collection index semantics. Typed write methods (`write_value_at`, `write_multiple`) shall be available under the same bound only. | 0.3 |
| `RSF-SF-FR-007` | For any `M: BorrowMut<FileMetadata>`, the component shall implement `SerialDataContainer<T>` by implementing its required reader and writer supertraits. | 0.3 |
| `RSF-SF-FR-008` | Insertion at an index less than the current length shall shift later records right without changing their order; insertion at the length shall append; a greater index shall fail. | 0.1 |
| `RSF-SF-FR-009` | Removal shall preserve the order of all remaining records, reduce the logical length by one on success, and return the removed decoded value for `serial_remove_get`. | 0.1 |
| `RSF-SF-FR-010` | Clear shall remove all logical rows while preserving the metadata prefix. Truncation shall retain the first requested number of rows and shall succeed without mutation when the requested length is at least the current length. | 0.1 |
| `RSF-SF-FR-011` | The component shall expose immutable access to the `FileMetadata` (`file_metadata(&self) -> &FileMetadata`) and let the caller recover the storage by consuming the view (`into_inner(self) -> M`). For owned storage, `into_file_metadata(self) -> FileMetadata` shall remain available. It shall not expose mutable access to the metadata that can invalidate its cached collection length while the view exists. | 0.3 |
| `RSF-SF-FR-012` | `serial_len` and `serial_is_empty` shall be available without fallible file I/O and reflect the count maintained by the `StructFile` instance. | 0.1 |
| `RSF-SF-FR-013` | The component shall provide `write_multiple(&[T])` to write the supplied records in slice order starting at row zero, replacing existing rows and appending any suffix that exceeds the current row count; it shall not sort or validate input ordering. | 0.1 |
| `RSF-SF-FR-014` | The component shall provide the public alias `StructReadOnlyFile<'a, T> = StructFile<T, &'a FileMetadata>`. It implements `SerialDataReader<T>` only; calling a write operation on it shall be a compile-time error. Several read-only views of the same `FileMetadata` may exist at once. | 0.3 |
| `RSF-SF-FR-015` | The component shall provide the public alias `StructWriteFile<'a, T> = StructFile<T, &'a mut FileMetadata>`. It implements `SerialDataReader<T>`, `SerialDataWriter<T>`, and `SerialDataContainer<T>`. After it is dropped, the borrowed `FileMetadata` reflects all its successful writes. | 0.3 |
| `RSF-SF-FR-016` | Every view shall provide `as_read_only(&self) -> StructReadOnlyFile<'_, T>`. It shall be infallible, perform no file I/O, and copy the view's cached length. No equivalent writable reborrow shall be provided, because writes through a reborrow would leave the parent view's cached length stale. | 0.3 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `RSF-SF-DS-001` | Each logical record shall occupy exactly `T::SIZE` bytes in row order after the `FileMetadata` metadata prefix. | 0.1 |
| `RSF-SF-DS-002` | Construction shall initialize the logical row count from `FileMetadata::element_count`; it shall reject a count that cannot be represented as `usize`. | 0.1 |
| `RSF-SF-DS-003` | Encoding shall use a row buffer of exactly `T::SIZE` bytes. A successful `try_encode` must report that exactly `T::SIZE` bytes were encoded before the row can be written. | 0.1 |
| `RSF-SF-DS-004` | Decoding shall use exactly one complete row and `T::try_decode`; incomplete rows shall not be returned as records. | 0.1 |
| `RSF-SF-DS-005` | The metadata prefix shall be preserved by all collection row mutations and shall not be interpreted or rewritten by `StructFile`. | 0.1 |
| `RSF-SF-DS-006` | Successful append, bulk write, insert, remove, clear, and truncate operations shall update the instance's logical length to match the resulting collection. Failed operations shall not report a successful length change. | 0.1 |
| `RSF-SF-DS-007` | Record order shall be the physical row order. The component shall not automatically sort records except when a caller explicitly uses a sorted insertion operation from `SerialDataContainer`. | 0.1 |
| `RSF-SF-DS-008` | Search and sorted-insertion behavior shall follow `SerialDataContainer` comparison semantics and shall propagate undefined ordering and read errors. | 0.1 |
| `RSF-SF-DS-009` | `write_multiple` shall preserve the order of its input slice without sorting or validating its ordering. The caller is responsible for providing records in the desired order. It shall replace rows `[0, min(slice.len(), current_len))`, append any remaining records contiguously, and leave existing rows after the slice unchanged. An empty slice shall perform no writes. | 0.1 |
| `RSF-SF-DS-010` | Each construction through `new` shall read the length from the metadata again, so a view created after an earlier view was dropped sees every change that view made. Views over the same `FileMetadata` shall share the file format and never keep a separate copy of the rows. | 0.3 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `RSF-SF-RS-001` | `StructFile<T, M>` shall be a generic type with `T: SerializedStruct`. It shall not implement `Clone`, for any `M`. | 0.3 |
| `RSF-SF-RS-002` | The reader iterator shall implement `Iterator<Item = anyhow::Result<T>>` and borrow the view for its lifetime. | 0.3 |
| `RSF-SF-RS-003` | Collection methods shall use the existing `SerialDataReader<T>`, `SerialDataWriter<T>`, and `SerialDataContainer<T>` trait signatures and `anyhow::Result` error convention. | 0.1 |
| `RSF-SF-RS-004` | Public index arguments use `usize`; conversions to `FileMetadata` row indices shall be checked and report overflow. | 0.1 |
| `RSF-SF-RS-005` | File length shall be stored as instance-local collection state so `serial_len` does not need to perform fallible I/O. | 0.1 |
| `RSF-SF-RS-006` | Row and shift buffers shall use fallible allocation where allocation is required; allocation failure shall be reported rather than converted to a panic or success. | 0.1 |
| `RSF-SF-RS-007` | Read operations, constructors, and accessors shall require only `M: Borrow<FileMetadata>`. Write operations shall require `M: BorrowMut<FileMetadata>`. The struct definition shall not bound `M`. `src/file/struct_file.rs` shall not import or name `FileLock`, its guards, or any synchronization type; lock guards work as storage only because they implement `Borrow`/`BorrowMut<FileMetadata>`, or through a reborrow (`&*guard`, `&mut *guard`). | 0.4 |
| `RSF-SF-RS-008` | The storage parameter shall be declared last with the default `M = FileMetadata`, so `StructFile<T>` keeps naming the owned type of revision 0.2. | 0.3 |
| `RSF-SF-RS-009` | `Send` and `Sync` shall be derived automatically from `T` and `M`; no manual `unsafe impl` shall be written. The type documentation shall state that `StructFile<T, M>` is `Send`/`Sync` when `T` and `M` are, and that `&FileMetadata` and `&mut FileMetadata` views are `Send` and `Sync` because `FileMetadata` is. | 0.3 |
| `RSF-SF-RS-010` | The documentation shall state that `M`'s `borrow`/`borrow_mut` must return the same `FileMetadata` for the view's whole lifetime. If a custom `M` breaks this, results may be wrong (for example a stale length), but the component shall stay memory-safe and still report out-of-range rows as errors through `FileMetadata`'s range checks. | 0.3 |

## Public API

The types shall be available from `rust_serial_file::file::struct_file` and
implement the adapter traits from `rust_serial_file::adapter`.

Representative API:

```rust
pub struct StructFile<T: SerializedStruct, M = FileMetadata> { /* private fields */ }
pub struct StructFileIter<'a, T: SerializedStruct> { /* private fields */ }

/// Read-only view over borrowed metadata; cannot write.
pub type StructReadOnlyFile<'a, T> = StructFile<T, &'a FileMetadata>;
/// Writable view over mutably borrowed metadata.
pub type StructWriteFile<'a, T> = StructFile<T, &'a mut FileMetadata>;

impl<T: SerializedStruct, M: Borrow<FileMetadata>> StructFile<T, M> {
    pub fn new(metadata: M) -> anyhow::Result<Self>;
    pub fn file_metadata(&self) -> &FileMetadata;
    pub fn into_inner(self) -> M;
    pub fn as_read_only(&self) -> StructReadOnlyFile<'_, T>;
    pub fn read_value_at(&self, index: usize) -> anyhow::Result<T>;
}

impl<T: SerializedStruct, M: BorrowMut<FileMetadata>> StructFile<T, M> {
    pub fn write_value_at(&mut self, index: usize, value: &T) -> anyhow::Result<()>;
    pub fn write_multiple(&mut self, values: &[T]) -> anyhow::Result<()>;
}

impl<T: SerializedStruct> StructFile<T, FileMetadata> {
    pub fn open_or_create(
        path: impl AsRef<std::path::Path>,
        data_offset: u64,
    ) -> anyhow::Result<Self>;
    pub fn into_file_metadata(self) -> FileMetadata;
}

impl<T, M: Borrow<FileMetadata>>    SerialDataReader<T>    for StructFile<T, M> { /* ... */ }
impl<T, M: BorrowMut<FileMetadata>> SerialDataWriter<T>    for StructFile<T, M> { /* ... */ }
impl<T, M: BorrowMut<FileMetadata>> SerialDataContainer<T> for StructFile<T, M> {}
```

Usage without any lock (single-threaded):

```rust
let mut metadata = FileMetadata::open_or_create(path, Record::SIZE as u64, 0)?;
{
    let mut writer = StructWriteFile::<Record>::new(&mut metadata)?;
    writer.serial_write(record)?;
}
let reader = StructReadOnlyFile::<Record>::new(&metadata)?;
let other_reader = StructReadOnlyFile::<Record>::new(&metadata)?;
assert_eq!(reader.serial_len(), other_reader.serial_len());
```

With a lock (multi-threaded), chosen by the caller and unknown to
`StructFile`:

```rust
// The view holds the guard, so the read lock lasts as long as `reader`.
let reader = StructFile::<Record, _>::new(file_lock.read()?)?;
// Or keep the guard separately and borrow from it.
let guard = file_lock.read()?;
let borrowed = StructReadOnlyFile::<Record>::new(&*guard)?;
```

`StructFileIter` is the associated owned-row iterator type. The reader
provides the trait's required `serial_len`, `serial_is_empty`, `serial_get`,
and `serial_iter` behavior through `&self`. Iteration yields one decoded value
per complete row in ascending file order; read or decode failures are yielded
as `Err` items and terminate iteration.

`SerialDataWriter<T>` shall implement, for writable views:

- `serial_write(value)`: encode and append one record.
- `serial_insert(index, value)`: insert before the existing row at `index`;
  `index == serial_len()` appends.
- `serial_write_at(index, value)`: replace one existing row.
- `serial_remove(index)`: remove one existing row.
- `serial_remove_get(index)`: remove and return one existing decoded row.
- `serial_clear()`: remove all rows but retain the metadata prefix.
- `serial_truncate(len)`: retain at most the first `len` rows.

The implementation may provide additional typed convenience methods, but
they shall not conflict with or weaken the trait behavior above. No method
shall return `&mut FileMetadata` from a view, because external mutations
would make the infallible trait length stale.

`write_multiple` is a bulk replacement beginning at row zero, not an insert
or unconditional append operation. It shall preserve slice order as supplied
by the caller and shall not sort or check record ordering. When the slice is
longer than the current collection, its existing-row prefix is replaced and
the excess suffix is appended contiguously; when shorter, rows after the
written prefix remain unchanged. An empty slice performs no writes.

## Errors and failure behavior

- Constructors shall return an error for zero `T::SIZE`, a row-width
  mismatch, failure to obtain the initial row count, or a row count that
  cannot fit in `usize`. This applies to every storage type, so a view built
  for the wrong record type is rejected.
- `as_read_only` cannot fail and performs no I/O.
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
  reports success. Because `FileMetadata` keeps its own row count, a later
  view built with `new` reads the count left by the failed operation.
- `serial_len` cannot return an error under the existing trait contract. It
  shall return the instance's last successfully maintained count; external
  mutations through other handles or processes while the view is alive are
  unsupported.
- No file, serialization, or decoding error shall be silently converted into
  a successful result.

## Safety and security

- None: no `unsafe` code is required or allowed. In particular, `Send` and
  `Sync` shall not be implemented manually (`RSF-SF-RS-009`).
- File contents are untrusted bytes. The implementation shall validate row
  size, index bounds, range arithmetic, serialization output length, and
  allocation sizes before using row data.
- The component shall rely on `SerializedStruct::try_decode` for typed
  decoding and shall not treat file bytes as trusted application values.
- The component shall not expose the underlying file handle or a mutable
  `FileMetadata` reference that could bypass collection length maintenance.
  `into_inner` returns the storage only by consuming the view, after which no
  cached length exists.
- Serialized values and temporary row buffers remain in process memory until
  released; secure zeroization is not required.
- `StructFile` does not synchronize access through other handles or
  processes, and has no locking of its own. In-process sharing across threads
  is the caller's choice of `M` (for example a lock guard). OS-level locking
  is deferred to the planned `FileAtlas`.

## Compatibility

- `FileMetadata` remains the owner and authority for file layout, metadata
  prefix, and row I/O. This component shall not alter its behavior.
- The existing `SerializedStruct` wire representation and the
  `SerialDataReader`, `SerialDataWriter`, and `SerialDataContainer` APIs shall
  be used without modification.
- **Source compatibility (revision 0.3):** `StructFile<T>` still names the
  owned type, and `new`, `open_or_create`, `file_metadata`,
  `into_file_metadata`, `read_value_at`, `write_value_at`, `write_multiple`,
  and all trait methods keep their signatures for owned storage. Existing
  code using `StructFile<T>` shall compile unchanged. Code that names
  `StructFileIter<'a, T>` is unaffected.
- **Possible inference change:** `StructFile::new(metadata)` without a type
  annotation now has an extra type parameter. When `M` is inferred from the
  argument this is invisible; the existing tests (T-001 to T-025) shall
  compile unchanged as a check.
- The file format consists of the existing opaque metadata prefix followed
  by fixed-width rows of `T::SIZE` bytes. No type tags, schema versions, or
  additional headers are added.
- `StructFile` shall preserve row order and bytes for records not affected
  by a successful collection mutation.
- Existing files opened with the matching row width and data offset require
  no migration. A row-width mismatch shall be rejected rather than
  reinterpreted.
- `FileLock` guards implement `Borrow`/`BorrowMut<FileMetadata>` since
  `FileLock` revision 0.5, so `StructFile<T, FileReadGuard<'_>>` and
  `StructFile<T, FileWriteGuard<'_>>` work with no change to this component.
  Building a view from a reborrow (`&*guard`, `&mut *guard`) keeps working.

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
- `serial_len`, `serial_is_empty`, and `as_read_only` shall not perform file
  I/O.
- Building a view with `new` costs one `FileMetadata::element_count` call (a
  file-length query) and the row-width checks. Views are meant to be cheap
  enough to build per use.
- Generic storage shall add no runtime cost beyond one `borrow` call per
  metadata access, which is inlined for references and owned values.
- The collection provides no internal locking. A view that holds a lock
  guard keeps the lock for its whole lifetime; long-lived read views can
  delay writers, so applications should build views when needed and drop
  them promptly. This is a usage guideline, not a guarantee.
- No throughput guarantee is made for insertions or removals; their
  required row shifting is proportional to the number of affected rows.

## Phases

### Phase planning and reflection

The number of project-specific phases is decided during this planning
reflection. The last two phases are always Coverage then Documentation.
Phases P-001 to P-005 were completed in revision 0.1 under the previous
template, where P-005 was a project-specific coverage phase. P-006 records
the revision 0.2 change. Revision 0.3 adds P-007 to P-009, so Coverage is
`P-010` and Documentation is `P-011`.

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Define the typed file layout, constructors, serialization checks, trait semantics, iterator behavior, and error policy. | None | This document defines the observable API, ownership, collection behavior, and failure cases. |
| P-002 | Implement `StructFile<T>`, construction and layout validation, typed row and bulk writes, and the fallible typed iterator. | P-001 | The type owns `FileMetadata`, rejects invalid row widths, and reads/writes exact `T::SIZE` rows with observable serialization and I/O errors. |
| P-003 | Implement `SerialDataReader<T>`, `SerialDataWriter<T>`, and `SerialDataContainer<T>` for `StructFile<T>`, including ordered shifts, removal, clear, and truncate. | P-002 | All trait methods preserve serial collection semantics and keep the maintained length consistent after successful operations. |
| P-004 | Add unit and integration tests for serialization, collection operations, boundaries, failures, iteration, and file compatibility; run format, lint, build, and test validation. | P-003 | All deterministic requirements have tests and all applicable project checks pass. |
| P-005 | Revision 0.1 coverage: measure production-module coverage and add tests for reachable uncovered behavior. | P-004 | Coverage is reported for the complete production module, with remaining uncovered code documented and justified. |
| P-006 | Revision 0.2: change `read_value_at`, `serial_get`, and `serial_iter` to `&self`. | P-005 | Done in commit `74e5e61`; all `StructFile` tests pass. |
| P-007 | **Generic storage:** add the `M = FileMetadata` parameter last; move `new`, `file_metadata`, `into_inner`, and `read_value_at` to an impl bounded by `M: Borrow<FileMetadata>`; move `write_value_at`, `write_multiple`, and the private `shift_rows_left` to `M: BorrowMut<FileMetadata>`; keep `open_or_create` and `into_file_metadata` on `StructFile<T, FileMetadata>`; bound the three trait impls as in the Public API. | P-006 | `cargo build --all-targets` succeeds; T-001 to T-025 compile without edits and pass; `struct_file.rs` names no lock type. |
| P-008 | **Views:** add the `StructReadOnlyFile` and `StructWriteFile` aliases, `as_read_only`, and the doc comments required by `RSF-SF-RS-009` and `RSF-SF-RS-010`, including a `compile_fail` doc test showing that a read-only view cannot write. | P-007 | The aliases and `as_read_only` are public and documented; `cargo test --doc` runs the `compile_fail` example. |
| P-009 | **View tests:** add T-026 to T-031. | P-008 | T-026 to T-031 are implemented and pass; `cargo fmt --check` and `cargo clippy --all-targets` report no warnings. |
| P-010 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-009 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-011 | **Documentation:** update this requirements document and the code comments of the touched files. | P-010 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [x] P-001 — `Specify StructFile contract` — Done
- [x] P-002 — `Implement typed file operations` — Done
- [x] P-003 — `Implement serial container traits` — Done
- [x] P-004 — `Test and validate StructFile` — Done
- [x] P-005 — `Revision 0.1 coverage` — Done
- [x] P-006 — `Shared-reference reads (revision 0.2)` — Done
- [x] P-007 — `Generic storage` — Done
- [x] P-008 — `Read-only and writable views` — Done
- [x] P-009 — `View tests` — Done
- [x] P-010 — `Coverage of touched files` — Done
- [x] P-011 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- *Why this breakdown:* P-007 is a pure refactor that must not change
  behavior; keeping it separate lets the unchanged T-001 to T-025 prove
  source compatibility before any new API exists. P-008 adds the new public
  surface on top. P-009 tests it.
- *Order and dependencies:* strictly sequential. P-007 can be verified alone
  (existing tests); P-008 needs the generic impls; P-009 needs the aliases.
- *Why one generic type instead of two structs:* the read path (row reads,
  iterator, length checks) is shared by both views. Two concrete structs
  would duplicate it or move it into a shared inner type. A generic storage
  parameter is the usual Rust pattern (`BufReader<R>`, `Cursor<T>`), keeps
  the owned case for free, and leaves room for a future atlas to return a
  view that holds its own guard, which a view over a plain reference cannot
  do (this needs `Borrow` impls on the guards; see Out of scope).
- *Risks and unknowns:*
  1. Type inference: an unannotated `StructFile::new(x)` must still infer
     `M`. If a call site in the tests fails to infer, it is a source break
     and must be recorded in Compatibility.
  2. The storage bound. The plan first used `Deref<Target = FileMetadata>`;
     this turned out not to cover the owned case (see the implementation
     reflection).
  3. `as_read_only` copies the cached length. If a later change added
     interior mutability to a view, the copy could go stale; this is ruled
     out by keeping views free of interior state.
  4. The planned `FileAtlas` may want views that outlive a borrow of the
     atlas. That needs owned guards in `FileLock` (for example over
     `Arc<FileLock>`), not a change to `StructFile`.

**Implementation reflection (update as work progresses):**

- Revision 0.3 was planned and implemented on 2026-11-11 on top of
  `74e5e61`. No phase was added, removed, or reordered.
- **Deviation from the approved plan (P-007):** the plan bounded storage by
  `M: Deref<Target = FileMetadata>` and `M: DerefMut`. That does not compile
  for the default owned storage, because `FileMetadata` does not dereference
  to itself. The bounds were changed to `M: Borrow<FileMetadata>` and
  `M: BorrowMut<FileMetadata>`. Std implements these for `FileMetadata`
  itself, `&FileMetadata`, `&mut FileMetadata`, `Box`, `Arc`, and `Rc`, so the
  approved design (owned default plus borrowed views) is unchanged.
  Consequence: `FileLock` guards implement `Deref`/`DerefMut` but not
  `Borrow`, so a view is built from a reborrow (`&*guard`, `&mut *guard`)
  rather than from the guard itself. `RSF-SF-FR-001`, `RSF-SF-FR-005` to
  `RSF-SF-FR-007`, `RSF-SF-RS-007`, `RSF-SF-RS-010`, the Public API, the
  Compatibility and Performance sections, and T-030 were updated to match,
  within revision 0.3 since it was not yet committed. Letting a view hold a
  guard directly needs `Borrow` impls in `file_lock.rs`; that is deferred.
- **P-007:** metadata access now goes through `file_metadata()` and a
  private `file_metadata_mut()`; `new` reads the row width and count through
  `Borrow::borrow`. T-001 to T-025 compiled and passed with no edits to
  `tests/struct_file.rs`, so the risk about type inference did not happen.
- **P-008:** the aliases and `as_read_only` were added. T-027 is a
  `compile_fail,E0599` doc test; a matching passing doc test on
  `StructWriteFile` uses the same imports, which shows the read-only example
  fails because the write method is missing, not because of a bad path.
- **P-009:** T-026 and T-028 to T-031 were added to `tests/struct_file.rs`
  and pass. `grep` confirms `src/file/struct_file.rs` names no `FileLock`,
  guard, or synchronization type.
- **P-010:** coverage measured; the only never-executed source line is the
  same unreachable 64-bit overflow arm as before.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `RSF-SF-FR-001`, `RSF-SF-RS-001` | Construct a `StructFile` from a `FileMetadata` with a matching row width. | Construction succeeds and owns the metadata; the caller can recover it after consuming the wrapper. |
| T-002 | `RSF-SF-FR-002`, `RSF-SF-FR-003` | Construct with zero-sized and mismatched serialized row widths. | Construction fails before any row operation or file mutation. |
| T-003 | `RSF-SF-FR-003` | Open/create a typed file with a metadata prefix. | The row width is `T::SIZE`, the offset is retained, and an absent path is created. |
| T-004 | `RSF-SF-FR-004`, `RSF-SF-DS-003`, `RSF-SF-DS-004` | Write and read typed values at existing row indices. | Values round-trip and exactly one `T::SIZE` row is read or written. |
| T-005 | `RSF-SF-FR-004`, `RSF-SF-DS-003` | Use a `SerializedStruct` implementation that reports an incorrect encoded length. | The operation returns an error and does not write the malformed row. |
| T-006 | `RSF-SF-FR-005`, `RSF-SF-RS-002` | Iterate all typed rows from a file. | Values are yielded in row order as owned results and iteration terminates after the captured range. |
| T-007 | `RSF-SF-FR-005` | Cause a row read failure during iteration. | The failure is yielded as an error and iteration terminates. |
| T-008 | `RSF-SF-FR-006`, `RSF-SF-DS-006` | Append one or more values through `SerialDataWriter`. | File rows and `serial_len` increase consistently after successful appends. |
| T-009 | `RSF-SF-FR-006`, `RSF-SF-FR-008` | Insert values at the beginning, middle, and end, and try an out-of-range index. | Existing records shift without changing order; end insertion appends; invalid insertion fails without changing contents. |
| T-010 | `RSF-SF-FR-006` | Replace an existing value and attempt replacement at `serial_len()`. | The existing row is updated; the one-past-end replacement returns an error. |
| T-011 | `RSF-SF-FR-006`, `RSF-SF-FR-009` | Remove first, middle, and last values, with and without returning the removed value. | Removed values are correct, remaining order is preserved, length decreases, and invalid indices fail. |
| T-012 | `RSF-SF-FR-006`, `RSF-SF-FR-010`, `RSF-SF-DS-005` | Clear a file with metadata and truncate a collection to shorter, equal, and larger lengths. | Rows are removed or retained according to trait semantics and metadata bytes remain unchanged. |
| T-013 | `RSF-SF-FR-007`, `RSF-SF-DS-008` | Use container search, update, and sorted insertion on ordered test records. | Trait defaults operate on file-backed records and preserve ordering. |
| T-014 | `RSF-SF-FR-012`, `RSF-SF-RS-005` | Query length and emptiness before and after successful and failed operations. | Length queries require no file I/O and change only after successful mutations. |
| T-015 | `RSF-SF-FR-011` | Access and recover the owned `FileMetadata`. | The immutable accessor does not permit mutation, and consuming recovery returns the metadata owner. |
| T-016 | `RSF-SF-DS-001`, `RSF-SF-DS-005`, `RSF-SF-DS-007` | Reopen an existing typed file with a metadata prefix and verify all rows. | Prefix bytes and row order remain unchanged, and no additional file-format header is introduced. |
| T-017 | `RSF-SF-FR-013`, `RSF-SF-DS-009` | Bulk-write a pre-ordered slice shorter than, equal to, and longer than the existing row count, including an empty slice. | The slice is written from row zero in its original order, excess values append contiguously, untouched trailing rows remain, and an empty slice does not mutate the file. |
| T-018 | `RSF-SF-FR-006`, `RSF-SF-DS-006` | Make `try_encode` fail during append and bulk write. | Errors are returned, file bytes are unchanged, and the cached length does not increase. |
| T-019 | `RSF-SF-FR-004`, `RSF-SF-FR-005`, `RSF-SF-FR-009` | Make `try_decode` fail during direct read, removal, and iteration. | Errors are returned, removal leaves the file-backed collection unchanged, and iteration terminates after its error. |
| T-020 | `RSF-SF-FR-009` | Remove a row from a collection large enough to require multiple shift-buffer chunks. | Rows spanning chunk boundaries retain their order and the logical length decreases correctly. |
| T-021 | `RSF-SF-RS-006` | Request an impossible row-buffer allocation. | The allocation error is propagated. |
| T-022 | `RSF-SF-DS-004` | Read a row larger than the fixed stack buffer. | The larger row is read and decoded correctly using the fallible allocation path. |
| T-023 | `RSF-SF-FR-005`, `RSF-SF-RS-002` | Iterate more rows than fit in the `BufferedReader` cache. | All rows are decoded in order across cache-window boundaries. |
| T-024 | `RSF-SF-FR-006`, `RSF-SF-RS-006` | Replace, append, and insert a record larger than the stack buffer. | Large-record writes use the fallible allocation path and preserve row order and values. |
| T-025 | `RSF-SF-FR-004`, `RSF-SF-FR-006`, `RSF-SF-FR-009` | Replace and remove-and-return at the current collection length. | Both methods return errors without changing row bytes or logical length. |
| T-026 | `RSF-SF-FR-001`, `RSF-SF-FR-005`, `RSF-SF-FR-014`, `RSF-SF-RS-007` | Build two `StructReadOnlyFile` views over the same `&FileMetadata` and use `serial_get`, `serial_iter`, `serial_find`, `file_metadata`, and `into_inner` on both; also build a read-only `StructFile<T, Arc<FileMetadata>>`. | Both views read the same records at the same time; `into_inner` returns the reference; the `Arc` view reads correctly. |
| T-027 | `RSF-SF-FR-014`, `RSF-SF-RS-007` | `compile_fail` doc test: call `serial_write` on a `StructReadOnlyFile`. | The example fails to compile, proving a read-only view cannot write. |
| T-028 | `RSF-SF-FR-006`, `RSF-SF-FR-007`, `RSF-SF-FR-015`, `RSF-SF-DS-010` | Through a `StructWriteFile` over `&mut FileMetadata`, append, insert, remove, and `serial_auto_insert`; drop it; build a new read-only view and then an owned `StructFile` from the same metadata. | The later views see every change and the right length; the metadata's own row count matches. |
| T-029 | `RSF-SF-FR-016` | Call `as_read_only` on an owned `StructFile` and on a `StructWriteFile`, then read through the returned views. | The read-only view returns the same length and records as its parent, and the call cannot fail. |
| T-030 | `RSF-SF-FR-005`, `RSF-SF-FR-006`, `RSF-SF-RS-007` | Integration with `FileLock`: build a `StructWriteFile` from `&mut *lock.write()?`, write records, drop it, then build two `StructReadOnlyFile` views from two `read()` guards held at once. | Writes through the write guard are visible through both read guards; the test lives outside `struct_file.rs`, which names no lock type. |
| T-031 | `RSF-SF-RS-009` | Compile-time assertions that `StructFile<Record>`, `StructReadOnlyFile<'_, Record>`, and `StructWriteFile<'_, Record>` are `Send` and `Sync`. | The assertions compile. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `tests/struct_file.rs` | `constructs_from_file_metadata` |
| T-002 | `tests/struct_file.rs` | `rejects_invalid_serialized_row_width` |
| T-003 | `tests/struct_file.rs` | `opens_or_creates_typed_file` |
| T-004 | `tests/struct_file.rs` | `reads_and_replaces_typed_values` |
| T-005 | `tests/struct_file.rs` | `rejects_incorrect_encoded_length` |
| T-006 | `tests/struct_file.rs` | `iterates_typed_rows_in_order` |
| T-007 | `tests/struct_file.rs` | `iterator_reports_row_errors` |
| T-008 | `tests/struct_file.rs` | `serial_write_appends_values` |
| T-009 | `tests/struct_file.rs` | `serial_insert_shifts_rows_and_checks_bounds` |
| T-010 | `tests/struct_file.rs` | `serial_write_at_replaces_existing_value` |
| T-011 | `tests/struct_file.rs` | `serial_remove_preserves_remaining_order` |
| T-012 | `tests/struct_file.rs` | `serial_clear_and_truncate_preserve_metadata` |
| T-013 | `tests/struct_file.rs` | `container_operations_use_typed_records` |
| T-014 | `tests/struct_file.rs` | `maintains_infallible_collection_length` |
| T-015 | `tests/struct_file.rs` | `exposes_and_recovers_owned_metadata` |
| T-016 | `tests/struct_file.rs` | `preserves_existing_file_layout` |
| T-017 | `tests/struct_file.rs` | `write_multiple_replaces_and_appends_in_slice_order` |
| T-018 | `tests/struct_file.rs` | `encode_errors_do_not_change_collection_length` |
| T-019 | `tests/struct_file.rs` | `decode_errors_are_reported_without_removing_rows` |
| T-020 | `tests/struct_file.rs` | `serial_remove_shifts_rows_across_multiple_chunks` |
| T-021 | `src/file/struct_file.rs` | `reports_row_buffer_allocation_failure` |
| T-022 | `tests/struct_file.rs` | `reads_rows_larger_than_the_stack_buffer` |
| T-023 | `tests/struct_file.rs` | `iterator_reads_across_buffer_cache_windows` |
| T-024 | `tests/struct_file.rs` | `writes_rows_larger_than_the_stack_buffer` |
| T-025 | `tests/struct_file.rs` | `rejects_out_of_range_replace_and_remove_get_without_mutation` |
| T-026 | `tests/struct_file.rs` | `read_only_views_share_borrowed_metadata` |
| T-027 | `src/file/struct_file.rs` | doc test on `StructReadOnlyFile` (`compile_fail`) |
| T-028 | `tests/struct_file.rs` | `writable_view_changes_are_seen_by_later_views` |
| T-029 | `tests/struct_file.rs` | `as_read_only_copies_length_without_io` |
| T-030 | `tests/struct_file.rs` | `views_work_through_file_lock_guards` |
| T-031 | `tests/struct_file.rs` | `views_are_send_and_sync` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-002 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-003 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-004 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-005 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-006 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-007 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-008 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-009 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-010 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-011 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-012 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-013 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-014 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-015 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-016 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-017 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-018 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-019 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-020 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-021 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-022 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-023 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-024 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-025 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-026 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-027 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-028 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-029 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-030 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |
| T-031 | 2026-11-11 | `74e5e61+dirty` | ✅ Success |

#### Notes

- T-001 to T-025 were first run on 2026-11-11 at `74e5e61` (clean tree) as a
  baseline for the revision 0.2 owned-only type: all passed.
- After P-009, all tests were run on 2026-11-11 at `74e5e61+dirty` (the
  revision 0.3 changes not yet committed) with `cargo test` from
  `rust_serial_file/`: 179 passed, 0 failed across all targets, including
  30 `StructFile` integration tests, T-021, and the two `struct_file.rs` doc
  tests (T-027 and its passing companion). The 30 integration tests include
  `struct_file_reads_through_a_shared_reference`, which belongs to T-015 of
  `doc/requirements/data_container/serial_read_container.md`.
  `cargo fmt --check` and `cargo clippy --all-targets` reported no warnings.
- T-001 to T-025 passed with no edits to their code, which verifies
  acceptance criterion 8.
- Before revision 0.3 the test table recorded only a status; the revision
  0.1 runs have no recorded date or hash.

#### Warning notes

- None.

### Acceptance criteria

1. `StructFile<T, M>` validates its row width against `T::SIZE` for every
   storage type and is exported through the file module.
2. Read-only and writable views are available as `StructReadOnlyFile` and
   `StructWriteFile`; writing through a read-only view does not compile.
3. `struct_file.rs` has no dependency on `FileLock` or any locking type, yet
   views work through `FileLock` guards.
4. Typed reads, writes, and iteration use the `SerializedStruct` fallible
   APIs and report malformed serialization and file errors.
5. `write_multiple` writes records from row zero in caller-supplied order,
   overwrites the existing prefix, and appends any excess suffix without
   sorting or validating record ordering.
6. Every view implements `SerialDataReader<T>`; writable views also
   implement `SerialDataWriter<T>` and `SerialDataContainer<T>`, with correct
   indexing, ordering, and length semantics.
7. Insertions and removals preserve the order of unaffected records and the
   metadata prefix.
8. Existing code using `StructFile<T>` compiles unchanged (T-001 to T-025
   need no edits).
9. Boundary and failure behavior is tested; `cargo fmt --check`,
   `cargo clippy --all-targets`, `cargo build`, and `cargo test` (including
   doc tests) pass.
10. Coverage is measured for the touched files and uncovered reachable
    behavior is tested or documented.

## Code coverage

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** Measured in P-010 over the full test suite. The
  table lists the production file touched by revision 0.3. The touched test
  file `tests/struct_file.rs` is test code and is not reported. Before
  revision 0.3 (at `e4f5d72+dirty`) the file measured 100.00% (29/29)
  functions, 94.47% (205/217) lines, and 82.89% (344/415) regions.

### Current coverage results

- **Tool and command:** `cargo-llvm-cov 0.9.1` with `llvm-tools-preview`, on
  the stable toolchain, run from `rust_serial_file/`: `cargo llvm-cov clean`,
  then `cargo llvm-cov --all-targets`, then
  `cargo llvm-cov report --show-missing-lines`. Branch coverage is `N/A`:
  `--branch` needs a nightly compiler.
- **Date of run:** 2026-11-11
- **Git hash:** `74e5e61+dirty`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `src/file/struct_file.rs` | 100.00% (32/32) | 94.58% (227/240) | 83.56% (361/432) | N/A |

### Coverage notes

- **`src/file/struct_file.rs`:** `--show-missing-lines` reports one source
  line never executed: line 385, the `Err` arm of `u64::try_from(index)` in
  `StructFileIter::next`. It is unreachable on 64-bit targets, where every
  `usize` fits in `u64`. The other 12 missed lines and 71 missed regions are
  per-instantiation counts: each record type and storage type (owned,
  `&FileMetadata`, `&mut FileMetadata`, `Arc`) compiles the generic methods
  separately, and not every instantiation runs every error path, even though
  another instantiation covers the same source. The second generic parameter
  did not make any source line newly unexecuted. No further plan.

## Out of scope

- Defining or changing the `SerializedStruct` wire format or derive macro.
- Changing `FileMetadata`, `FileLock`, `SerialDataReader`, `SerialDataWriter`,
  or `SerialDataContainer` behavior.
- The `FileAtlas` itself: storing `FileLock`s, opening files, OS file locks,
  and the API that hands out typed views. It shall get its own requirements
  document. Owner: the user.
- Views that outlive a borrow of their lock (owned guards over
  `Arc<FileLock>`). This belongs to `FileLock` or `FileAtlas`, not here.
  Owner: the user, when the atlas is specified.
- Implementing `Clone` or `Copy` for read-only views. It would be safe for
  `&FileMetadata` storage but is not needed yet; `as_read_only` covers the
  known use. Owner: the user, if a use appears.
- A writable reborrow (`as_writable`) from a view, for the reason given in
  `RSF-SF-FR-016`.
- Concurrent mutation through multiple file handles or processes.
- Transactional rollback for partial file writes or row shifts.
- Automatic sorting except where explicitly requested through the sorted
  `SerialDataContainer` operations.
- Schema evolution, type identification, version headers, checksums, and
  migration of existing files.
- Returning mutable references to records stored in the file; records are
  read as owned decoded values.
