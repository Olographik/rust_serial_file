# File Lock Requirements

- **Date:** 2026-11-11
- **Author:** `Olographik`
- **Revision:** 0.6
- **Short description:** Define a `FileLock` that owns a `FileMetadata` behind a reader-writer lock, granting many concurrent read borrows or one exclusive write borrow, each held until its guard is released, with guards usable directly as `StructFile` storage.

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

`FileMetadata` (`rust_serial_file/src/file/file_metadata.rs`) is a
single-owner wrapper around a `std::fs::File`. Its read operations
(`read_row`, `read_rows`, `get_raw_metadata`, `element_count`, `iter_rows`,
`rows`, ...) take `&self` and use positional reads, while its mutating
operations (`write_row`, `append_row`, `insert_row`, `truncate_rows`,
`update_raw_metadata`, `reserve_rows`, ...) take `&mut self`. On its own, the
type cannot be shared between threads while still allowing mutation.

`FileLock` closes that gap. It takes ownership of a `FileMetadata` and stores it
inside a `std::sync::RwLock`. Callers borrow the file through the lock:

- a **read borrow** returns a `FileReadGuard` that gives `&FileMetadata`; any
  number of read borrows can exist at the same time;
- a **write borrow** returns a `FileWriteGuard` that gives `&mut FileMetadata`;
  only one write borrow can exist, and never at the same time as a read
  borrow.

The file stays locked (shared for reads, exclusive for writes) for as long as
the guard is alive, and is released when the guard is dropped. The intended
users are downstream components (for example a shared `StructFile` or a
multi-threaded reader and writer) that need to share one row file across
threads with `Arc<FileLock>`.

> **Caution: in-process locking only.** `FileLock` does not take any
> operating-system file lock and is not aware of other processes or of other
> handles opened on the same path. Operating-system locking is planned to be
> owned by a future `FileAtlas` component, which will own every opened file
> and check and acquire the OS lock when a file is opened. `FileLock` has no
> dependency on `FileAtlas` and will not change when it is added. Until
> `FileAtlas` exists, or when `FileLock` is used without it, the caller is
> responsible for making sure that no other process or handle reads or writes
> the same file while it is in use.

If a thread panics while it holds a write guard, the lock becomes poisoned.
`FileLock` then reports errors instead of handing out guards, and keeps a
`PoisonReport` that compares the file state when the write guard was acquired
with the state when it was dropped by the panic. The caller inspects that
report with `recover()`, which also clears the poison, and decides whether to
finish the interrupted work, undo it, or reopen the file from disk. A
generation counter, increased each time a write guard is released normally,
lets callers check whether write sessions completed.

Since revision 0.5, the guards also implement `Borrow<FileMetadata>` (and the
write guard `BorrowMut<FileMetadata>`). `StructFile<T, M>` accepts any `M`
with those bounds, so a typed view can **hold its guard directly**, for
example `StructFile<Record, FileReadGuard<'_>>`, and keeps the lock for as
long as the view lives. Before, a view could only be built from a reborrow
(`&*guard`), so the guard and the view had to be kept side by side. This lets
a future `FileAtlas` hand out a single value that is both the lock and the
typed view. `FileLock` still does not know about `StructFile`.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-11 | `Olographik` | Added `FL-RS-006` (doc comments warn that locking is in-process only). Resolved the operating-system lock decision: it belongs to a future `FileAtlas`, not to `FileLock`. Added the in-process caution to Overview, Assumptions, and Safety, and updated the P-006 completion criteria, planning reflection, and Out of scope. |
| 0.3 | 2026-11-11 | `Olographik` | Poison recovery and tracking. Added `FL-FR-013` (write-guard snapshot and `PoisonReport`), `FL-FR-014` (`recover`), `FL-FR-015` (generation counter), `FL-FR-016` (limits of the report), and `FL-RS-007` (guard `Drop` never panics). Modified `FL-FR-012` (errors until `recover`), `FL-DS-002` (write acquisition reads the file length), and `FL-RS-003` (atomic generation counter allowed). Added phase P-005 (poison tracking and recovery); Coverage and Documentation renumbered to P-006 and P-007. Added tests T-019 to T-023. Updated Overview, Public API, Errors, Performance, planning reflection, and Out of scope (poison recovery resolved; mid-operation I/O errors recorded as an open issue). |
| 0.4 | 2026-11-11 | `Olographik` | Buffered reader and writer integration. Added `FL-FR-017` (read guard usable with `BufferedReader`), `FL-FR-018` (write guard usable with `BufferedWriter`), and `FL-FR-019` (a `BufferedWriter` borrowing the write guard is flushed before the lock is released, including during unwinding). Added phase P-006 (buffered integration tests); Coverage and Documentation renumbered to P-007 and P-008 and redone. Added tests T-024 to T-030. Updated Scope, Compatibility, implementation reflection, and notes. |
| 0.5 | 2026-11-11 | `Olographik` | Guards usable as `StructFile` storage. Added `FL-FR-020` (`Borrow<FileMetadata>` for `FileReadGuard`), `FL-FR-021` (`Borrow` and `BorrowMut<FileMetadata>` for `FileWriteGuard`), `FL-FR-022` (a `StructFile` holding a guard keeps the lock for its lifetime), and `FL-RS-008` (`Borrow` impls agree with `Deref`, no `unsafe`, no dependency on `StructFile`). Added phases P-007 (`Borrow` impls) and P-008 (`StructFile` integration tests); Coverage and Documentation renumbered to P-009 and P-010 and unchecked. Added tests T-031 to T-034. Updated Overview, Scope, Public API, Compatibility, planning reflection, and Out of scope. |
| 0.6 | 2026-11-11 | `Olographik` | Documentation only; no requirement IDs changed. The OS-lock Out of scope item now points to `doc/requirements/file/file_atlas.md`, which specifies `FileAtlas` (exclusive `File::try_lock` on open). The two doc comments in `src/file/file_lock.rs` that called `FileAtlas` "planned" now link to it; no code changed. |

## Goals and scope

### Goals

- Allow several readers to access the same `FileMetadata` concurrently.
- Allow exactly one writer at a time, excluding all readers while it holds the
  lock.
- Tie the lifetime of the lock to an RAII guard, so that the lock is released
  exactly when the guard is dropped, including on early return and on panic
  unwinding.
- Make `FileLock` shareable between threads (`Send + Sync`) so it can be
  placed in an `Arc`.
- Report lock failures (poisoning, contention on non-blocking attempts) as
  errors instead of panicking.
- After a poisoning panic, let the caller see what changed during the
  interrupted write session and explicitly recover the lock.

### Scope

- New module `rust_serial_file/src/file/file_lock.rs`, exported from
  `rust_serial_file/src/file/mod.rs` as `pub mod file_lock;`.
- Types `FileLock`, `FileReadGuard<'a>`, and `FileWriteGuard<'a>`.
- Blocking acquisition (`read`, `write`), non-blocking acquisition
  (`try_read`, `try_write`), ownership operations (`new`,
  `open_or_create`, `get_mut`, `into_inner`), and poison tracking and recovery
  (`FileSnapshot`, `PoisonReport`, `recover`, `generation`).
- `FileMetadata`, `StructFile`, `BufferedReader`, and `BufferedWriter` must
  remain unchanged in behavior and public API. `BufferedReader` and
  `BufferedWriter` borrow a `FileMetadata` (`&FileMetadata` and
  `&mut FileMetadata`) and do not take ownership; they are used with
  `FileLock` by borrowing through a guard (`FL-FR-017` to `FL-FR-019`). `FileLock` only composes
  `FileMetadata`; it does not reimplement or wrap its row operations.
- Revision 0.5 adds `Borrow<FileMetadata>` and `BorrowMut<FileMetadata>`
  impls to the guards in `src/file/file_lock.rs`, so `StructFile` (which
  needs `M: Borrow<FileMetadata>` to read and `M: BorrowMut<FileMetadata>`
  to write) can hold a guard. `StructFile`, `FileMetadata`, and the buffered
  types are not changed. `doc/requirements/file/struct_file.md` is updated to
  record that its deferred item is resolved.

## Assumptions and dependencies

- Locking is **in-process**: it coordinates threads of the same process that
  share one `FileLock`. It does not prevent another process, or another
  `FileMetadata` opened on the same path, from accessing the file.
  Operating-system file locks are the responsibility of a future `FileAtlas`
  component (see [Out of scope](#out-of-scope)); `FileLock` does not depend on
  it. Without `FileAtlas`, the caller must guarantee exclusive use of the file
  by this process.
- `FileMetadata` is `Send + Sync` (it holds a `File` and plain `u64` fields),
  and its `&self` read operations use positional I/O (`read_exact_at`), so
  concurrent shared reads through `&FileMetadata` do not race on a shared
  file cursor in the data they return.
- Dependencies: the standard library (`std::sync::RwLock`,
  `RwLockReadGuard`, `RwLockWriteGuard`, `TryLockError`) and `anyhow`, which
  is already a dependency of the crate. No new crate is required.
- `tempfile` (existing dev-dependency) is used by the tests.
- Rust edition 2024, as declared in `rust_serial_file/Cargo.toml`. No MSRV is
  declared for the crate; the implementation must not need a feature newer
  than the edition 2024 baseline (Rust 1.85).

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `FL-FR-001` | `FileLock::new(metadata: FileMetadata) -> FileLock` takes ownership of an existing `FileMetadata` and stores it unlocked. It is infallible. | 0.1 |
| `FL-FR-002` | `FileLock::open_or_create(path, element_size, data_offset) -> Result<FileLock>` builds the inner value with `FileMetadata::open_or_create` and forwards its errors unchanged (for example `element_size == 0` or an open failure). | 0.1 |
| `FL-FR-003` | `FileLock::read(&self) -> Result<FileReadGuard<'_>>` acquires a shared lock, blocking while a write guard is alive. Several read guards obtained from the same `FileLock`, on the same or different threads, can be alive at the same time. | 0.1 |
| `FL-FR-004` | `FileLock::write(&self) -> Result<FileWriteGuard<'_>>` acquires an exclusive lock, blocking while any read or write guard is alive. While a write guard is alive, no other read or write guard can be obtained. | 0.1 |
| `FL-FR-005` | `FileReadGuard` dereferences to `&FileMetadata` (`Deref<Target = FileMetadata>`) and gives no mutable access. | 0.1 |
| `FL-FR-006` | `FileWriteGuard` dereferences to `&FileMetadata` and `&mut FileMetadata` (`Deref` and `DerefMut` with `Target = FileMetadata`). | 0.1 |
| `FL-FR-007` | Dropping a guard releases its lock. Once all read guards are dropped, a blocked `write` call proceeds; once the write guard is dropped, blocked `read` or `write` calls proceed. Release also happens when the guard is dropped during panic unwinding. | 0.1 |
| `FL-FR-008` | `FileLock::try_read(&self) -> Result<Option<FileReadGuard<'_>>>` and `FileLock::try_write(&self) -> Result<Option<FileWriteGuard<'_>>>` never block: they return `Ok(Some(guard))` when the lock is acquired and `Ok(None)` when it is currently held in a conflicting mode. | 0.1 |
| `FL-FR-009` | Changes made through a write guard (rows written, appended, inserted, truncated, metadata updated) are visible to every guard acquired after that write guard is dropped. | 0.1 |
| `FL-FR-010` | `FileLock::get_mut(&mut self) -> Result<&mut FileMetadata>` gives mutable access without locking, since exclusive access is proven by the borrow checker. | 0.1 |
| `FL-FR-011` | `FileLock::into_inner(self) -> Result<FileMetadata>` consumes the lock and returns the owned `FileMetadata`. | 0.1 |
| `FL-FR-012` | If a thread panics while holding a write guard, every later `read`, `write`, `try_read`, `try_write`, `get_mut`, and `into_inner` call returns an `Err` stating that the file lock is poisoned, and never panics, until the poison is cleared by `recover` (`FL-FR-014`). | 0.3 |
| `FL-FR-013` | `write` and `try_write` record a `FileSnapshot` (`file_len`, `element_count`, `data_offset`) of the file when the write guard is acquired. If the write guard is dropped while its thread is panicking (`std::thread::panicking()`), it stores a `PoisonReport` in the `FileLock` containing the acquisition snapshot (`before`), a snapshot taken at drop (`after`, `None` if it cannot be read), and the name of the panicking thread (`None` if unnamed). A write guard dropped normally stores no report. | 0.3 |
| `FL-FR-014` | `FileLock::recover(&self) -> Result<Option<PoisonReport>>` blocks like `write`. On a poisoned lock it removes and returns the stored `PoisonReport`, clears the poison, and returns `Ok(Some(report))`; later calls then behave as on a lock that was never poisoned. On a lock that is not poisoned it returns `Ok(None)` and changes nothing. `recover` does not repair or change the file or the `FileMetadata`. | 0.3 |
| `FL-FR-015` | `FileLock::generation(&self) -> u64` returns a counter that starts at 0 and is increased by one each time a write guard is dropped while its thread is not panicking. Read guards, `get_mut`, and write guards dropped by a panic do not change it. It is readable even when the lock is poisoned. | 0.3 |
| `FL-FR-016` | The `PoisonReport` only describes changes in file length, element count, and data offset. Rows overwritten in place (same length and count) are not detected, and this limit is stated in the doc comments of `PoisonReport` and `recover`. | 0.3 |
| `FL-FR-017` | A `BufferedReader` can be built from a read guard (`BufferedReader::new(&guard)` or `with_capacity`) through `Deref`, without any change to `BufferedReader`. It borrows the guard, so the read lock is held while the reader is alive; several buffered readers, each on its own read guard, can run at the same time on different threads. | 0.4 |
| `FL-FR-018` | A `BufferedWriter` can be built from a write guard (`BufferedWriter::new(&mut guard)` or `with_capacity`) through `DerefMut`, without any change to `BufferedWriter`. It borrows the guard mutably, so the write lock is held while the writer is alive, and the guard can be used again once the writer is dropped. | 0.4 |
| `FL-FR-019` | Because a `BufferedWriter` borrows its write guard, it is always dropped before the guard, so its `Drop` flush writes pending rows to the file before the write lock is released; readers acquiring the lock afterwards see them. During panic unwinding the same order holds, so the `after` snapshot of the `PoisonReport` includes the flushed rows. Flush errors in `BufferedWriter::drop` stay ignored as today; callers that need to observe them call `flush()` explicitly before dropping the writer. | 0.4 |
| `FL-FR-020` | `FileReadGuard` implements `Borrow<FileMetadata>`. `borrow()` returns the same `FileMetadata` as `Deref`. It gives no mutable access; `FileReadGuard` does not implement `BorrowMut`. | 0.5 |
| `FL-FR-021` | `FileWriteGuard` implements `Borrow<FileMetadata>` and `BorrowMut<FileMetadata>`. `borrow()` and `borrow_mut()` return the same `FileMetadata` as `Deref` and `DerefMut`. Changes made through `borrow_mut()` follow the same rules as changes through `DerefMut`: they are visible after the guard is dropped (`FL-FR-009`), and the drop updates the generation counter or stores a `PoisonReport` as before (`FL-FR-013`, `FL-FR-015`). | 0.5 |
| `FL-FR-022` | As a result of `FL-FR-020` and `FL-FR-021`, `StructFile<T, FileReadGuard<'_>>` is a read-only typed view and `StructFile<T, FileWriteGuard<'_>>` a writable one, without any change to `StructFile`. A view holding a guard keeps the lock for its whole lifetime and releases it when the view is dropped. `StructFile::into_inner` returns the guard, which can then be used or dropped as any guard. | 0.5 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `FL-DS-001` | `FileLock` adds no on-disk data: it writes no lock marker, header, or sidecar file, and the file layout is exactly the one defined by `FileMetadata`. | 0.1 |
| `FL-DS-002` | Acquiring or releasing a guard never reads or writes file contents and never changes the file or the cached `FileMetadata` state (element count, reserved rows, offsets). Acquiring a read guard performs no I/O. Acquiring a write guard, and dropping a write guard while panicking, only query the file length for the `FileSnapshot`. | 0.3 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `FL-RS-001` | `FileLock` is `Send + Sync`, so it can be shared with `Arc<FileLock>`; this is checked by a compile-time assertion in the tests. | 0.1 |
| `FL-RS-002` | `FileReadGuard<'a>` and `FileWriteGuard<'a>` borrow the `FileLock` for `'a`, so a guard cannot outlive its lock. They wrap the standard library guards in newtypes so the standard library guard types are not part of the public API. | 0.1 |
| `FL-RS-003` | Read access is only possible through `&FileMetadata` from a read guard, and mutation only through `&mut FileMetadata` from a write guard or `get_mut`. No interior mutability is added other than the `RwLock` (which also holds the stored `PoisonReport`, so the write guard can store it without another lock) and an `AtomicU64` for the generation counter. | 0.3 |
| `FL-RS-004` | Borrowed helpers of `FileMetadata` (for example `iter_rows` and `rows`, returning `FileRowIter<'_>` and `FileRows<'_>`) can be used through a read guard, and the borrow checker prevents them from outliving the guard. | 0.1 |
| `FL-RS-005` | The module uses Rust edition 2024 and contains no `unsafe` code. | 0.1 |
| `FL-RS-006` | The doc comments of `FileLock`, `read`, and `write` state that the lock only coordinates threads sharing this `FileLock`, that it takes no operating-system file lock, that OS locking is expected to be handled by the component owning the opened files (the planned `FileAtlas`), and that without it the caller must ensure no other process or handle accesses the file. | 0.2 |
| `FL-RS-007` | The `Drop` implementation of `FileWriteGuard` never panics: it uses no `unwrap` or `expect`, and a failure while taking the `after` snapshot is recorded as `after: None`. `FileSnapshot` and `PoisonReport` implement `Debug`, `Clone`, `PartialEq`, and `Eq`. | 0.3 |
| `FL-RS-008` | The `Borrow` and `BorrowMut` impls delegate to the existing `Deref` and `DerefMut` impls and contain no `unsafe` code. `FileMetadata` implements none of `Eq`, `Ord`, or `Hash`, so the `Borrow` contract about consistent comparison and hashing has nothing to violate. `src/file/file_lock.rs` shall not import or name `StructFile` or `SerializedStruct`. | 0.5 |

## Public API

Module `rust_serial_file::file::file_lock`:

```rust
pub struct FileLock {
    /* RwLock<{ FileMetadata, Option<PoisonReport> }>, AtomicU64 generation */
}

impl FileLock {
    pub fn new(metadata: FileMetadata) -> Self;
    pub fn open_or_create(
        path: impl AsRef<Path>,
        element_size: u64,
        data_offset: u64,
    ) -> Result<Self>;

    pub fn read(&self) -> Result<FileReadGuard<'_>>;
    pub fn write(&self) -> Result<FileWriteGuard<'_>>;
    pub fn try_read(&self) -> Result<Option<FileReadGuard<'_>>>;
    pub fn try_write(&self) -> Result<Option<FileWriteGuard<'_>>>;

    pub fn get_mut(&mut self) -> Result<&mut FileMetadata>;
    pub fn into_inner(self) -> Result<FileMetadata>;

    pub fn recover(&self) -> Result<Option<PoisonReport>>;
    pub fn generation(&self) -> u64;
}

impl From<FileMetadata> for FileLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub file_len: u64,
    pub element_count: u64,
    pub data_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoisonReport {
    pub before: FileSnapshot,          // when the write guard was acquired
    pub after: Option<FileSnapshot>,   // when it was dropped by the panic
    pub thread_name: Option<String>,
}

pub struct FileReadGuard<'a> { /* RwLockReadGuard<'a, FileMetadata> */ }
impl Deref for FileReadGuard<'_> { type Target = FileMetadata; }
impl Borrow<FileMetadata> for FileReadGuard<'_> {}          // revision 0.5

pub struct FileWriteGuard<'a> { /* RwLockWriteGuard<'a, FileMetadata> */ }
impl Deref for FileWriteGuard<'_> { type Target = FileMetadata; }
impl DerefMut for FileWriteGuard<'_> {}
impl Borrow<FileMetadata> for FileWriteGuard<'_> {}         // revision 0.5
impl BorrowMut<FileMetadata> for FileWriteGuard<'_> {}      // revision 0.5
```

Typed views holding a guard (revision 0.5):

```rust
let lock = FileLock::open_or_create(path, Record::SIZE as u64, 0)?;
{
    let mut writer = StructFile::<Record, _>::new(lock.write()?)?;
    writer.serial_write(record)?;
}                                   // view and write lock dropped together
let reader = StructFile::<Record, _>::new(lock.read()?)?;
let value = reader.serial_get(0)?;  // read lock held while `reader` lives
```

`Result` is `anyhow::Result`, as in the rest of the crate. Usage example:

```rust
let lock = Arc::new(FileLock::open_or_create(path, 16, 0)?);

{
    let mut file = lock.write()?;   // exclusive: blocks readers
    file.append_row(&[0u8; 16])?;
}                                   // write lock released here

let reader = Arc::clone(&lock);
std::thread::spawn(move || -> anyhow::Result<()> {
    let file = reader.read()?;      // shared: other readers allowed
    let mut row = [0u8; 16];
    file.read_row(0, &mut row)?;
    Ok(())
});                                 // read lock released when `file` drops

// After a writer panicked:
if let Some(report) = lock.recover()? {
    // e.g. before.element_count = 100, after.element_count = 101:
    // finish the interrupted work, undo it, or drop the lock and reopen.
}
```

## Errors and failure behavior

- **Poisoning:** if a thread panics while holding a write guard, the
  `RwLock` becomes poisoned because the `FileMetadata` may be in a partially
  updated state (for example during `insert_row`, which shifts data). `read`,
  `write`, `try_read`, `try_write`, `get_mut`, and `into_inner` then return an
  `Err` whose message states that the file lock is poisoned and that
  `recover` can be used. They must not panic and must not silently return the
  possibly inconsistent value. The poison is cleared only by `recover`, which
  returns the `PoisonReport` (`FL-FR-013`, `FL-FR-014`). Dropping the
  `FileLock` and reopening the path with `FileMetadata::open_or_create`,
  which rebuilds the cached counts from the file length, remains available as
  a fallback.
- **What the report cannot see:** rows overwritten in place (`FL-FR-016`),
  and I/O errors returned in the middle of a multi-step `FileMetadata`
  operation (`insert_row`, `truncate_rows_front`, `update_raw_metadata`).
  Those errors do not panic, so they do not poison the lock, but they can
  leave the file partially changed. This is an existing `FileMetadata`
  limitation, recorded in [Out of scope](#out-of-scope).
- **Contention on non-blocking calls:** `try_read` and `try_write` return
  `Ok(None)` when the lock is held in a conflicting mode. Contention is not an
  error. A poisoned lock is still reported as `Err`, even from `try_*`.
- **Constructor errors:** `open_or_create` forwards every
  `FileMetadata::open_or_create` error unchanged.
- **Operation errors:** errors from `FileMetadata` operations called through a
  guard are returned by those operations as today; the guard does not change
  them.
- **Re-entrancy:** acquiring a write guard on a thread that already holds a
  read or write guard on the same `FileLock` (or a read guard while holding
  the write guard) may deadlock or panic, as documented for
  `std::sync::RwLock`. This is a caller error; it is documented on `read` and
  `write`, and not detected at runtime.
- Releasing a guard cannot fail.

## Safety and security

- None: no `unsafe` code is allowed. `Send` and `Sync` are derived
  automatically from `RwLock<FileMetadata>`.
- No additional input validation: inputs are validated by `FileMetadata`.
- No sensitive data is handled beyond the file contents owned by
  `FileMetadata`.
- Threat assumption: the lock only coordinates threads that share this
  `FileLock`. It gives no protection against other processes or other handles
  on the same file, and must not be documented as doing so. Protection against
  other processes is planned in the future `FileAtlas`; until then, using a
  `FileLock` on a file that another process can open is a caller
  responsibility (`FL-RS-006`).

## Compatibility

- The public API of `FileMetadata`, `StructFile`, `BufferedReader`, and
  `BufferedWriter` is unchanged. The only change outside the new file is the
  `pub mod file_lock;` line in `src/file/mod.rs`.
- Revision 0.5 is additive: two new trait impls on each guard type, and no
  signature changes. One theoretical source break: downstream code that calls
  `guard.borrow()` with `std::borrow::Borrow` in scope and relies on inference
  to pick the blanket `Borrow<Self>` impl may now need a type
  annotation, because a second `Borrow` impl exists. No code in this
  workspace does that.
- Building a `StructFile` from a reborrow (`&*guard`, `&mut *guard`), the
  only option before revision 0.5, keeps working.
- `BufferedReader` and `BufferedWriter` work with `FileLock` without any
  change, by borrowing the `FileMetadata` through a read or write guard
  (`FL-FR-017`, `FL-FR-018`).
- The on-disk format is unchanged (`FL-DS-001`): a file written through a
  `FileLock` can be reopened with a plain `FileMetadata`, and the other way
  round.
- No migration is required.

## Performance and resource use

- Read-guard acquisition and release cost one `RwLock` operation, with no
  allocation and no file I/O. Write-guard acquisition also queries the file
  length for the `FileSnapshot` (two `metadata()` system calls, one from
  `file_len` and one from `element_count`); a normal
  write-guard release adds one atomic increment (`FL-DS-002`, `FL-FR-015`).
  Only a write guard dropped by a panic allocates (the thread name) and
  queries the file length again.
- Concurrency: readers run in parallel; a writer has exclusive access. The
  fairness and writer-starvation policy is that of `std::sync::RwLock` on the
  target platform and is not guaranteed by `FileLock`.
- `FileMetadata` read operations on Windows use `seek_read`, which also moves
  the file cursor. This has no effect on the data returned by positional
  reads, but no component may depend on the cursor position while read guards
  are shared.
- Throughput and latency targets are non-goals for this revision.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Type and ownership:** create `src/file/file_lock.rs`, export it from `src/file/mod.rs`, implement `FileLock` with `new`, `From<FileMetadata>`, `open_or_create`, `get_mut`, and `into_inner`, including the poisoning error mapping. Add the `Send + Sync` compile-time assertion. | None | The crate builds; T-001, T-002, T-003, T-004, and T-005 pass. |
| P-002 | **Blocking guards:** implement `FileReadGuard` and `FileWriteGuard` newtypes with `Deref` / `DerefMut`, and `read` / `write`. | P-001 | T-006, T-007, T-008, T-009, and T-010 pass. |
| P-003 | **Non-blocking guards and poisoning:** implement `try_read` and `try_write` with the `Ok(None)` contention result, and map poisoning on every acquisition path. | P-002 | T-011, T-012, T-013, and T-014 pass. |
| P-004 | **Multi-threaded tests:** add the thread-based tests that check concurrent readers, writer exclusion, release on drop, and on-disk compatibility. | P-003 | T-015, T-016, T-017, and T-018 pass reliably on 10 repeated runs; `cargo fmt --check`, `cargo clippy`, and `cargo test` pass. |
| P-005 | **Poison tracking and recovery:** add `FileSnapshot` and `PoisonReport`, take the snapshot in `write` / `try_write`, store the report from the write guard's `Drop` when panicking, implement `recover` (`PoisonError::into_inner` and `RwLock::clear_poison`) and the `generation` counter. | P-003 | T-019, T-020, T-021, T-022, and T-023 pass; T-014 still passes. |
| P-006 | **Buffered integration tests:** add tests that build `BufferedReader` from read guards and `BufferedWriter` from write guards, including concurrency, flush-on-drop ordering, and poisoning with a live writer. No change to the buffered files is allowed; if the API did not work, the gap is recorded here instead. | P-005 | T-024 to T-030 pass on 10 repeated runs; `src/file/buffered_reader.rs` and `src/file/buffered_writer.rs` are unchanged. |
| P-007 | **`Borrow` impls:** add `impl Borrow<FileMetadata> for FileReadGuard<'_>`, and `impl Borrow<FileMetadata>` and `impl BorrowMut<FileMetadata>` for `FileWriteGuard<'_>`, each delegating to `Deref` / `DerefMut`, with doc comments naming `StructFile` storage as the purpose. | P-006 | The crate builds; T-001 to T-030 pass unchanged; `file_lock.rs` names no `StructFile` or `SerializedStruct`; T-031 passes. |
| P-008 | **`StructFile` integration tests:** add tests that use guards directly as `StructFile` storage: concurrent read views, a write view released on drop, `into_inner`, and poisoning with a live write view. | P-007 | T-032 to T-034 pass on 10 repeated runs; `src/file/struct_file.rs` is unchanged. |
| P-009 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-008 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-010 | **Documentation:** update this requirements document, the code comments of the touched files, and `doc/requirements/file/struct_file.md` (its deferred `Borrow` item, Overview, and Compatibility). | P-009 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments, including the in-process-only caution of `FL-RS-006` and the re-entrancy warning; `struct_file.md` no longer lists the guard `Borrow` impls as deferred. |

### Current implementation checklist

- [x] P-001 — `Type and ownership` — Done
- [x] P-002 — `Blocking guards` — Done
- [x] P-003 — `Non-blocking guards and poisoning` — Done
- [x] P-004 — `Multi-threaded tests` — Done
- [x] P-005 — `Poison tracking and recovery` — Done
- [x] P-006 — `Buffered integration tests` — Done
- [x] P-007 — `Borrow impls on guards` — Done
- [x] P-008 — `StructFile integration tests` — Done
- [x] P-009 — `Coverage of touched files` — Done
- [x] P-010 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- *Why is this phase breakdown appropriate for the project?* The component is
  small and composes `std::sync::RwLock`. The phases follow the dependency
  chain of the API: the owning type first, then the guards that borrow from
  it, then the non-blocking variants that reuse the guard types. The
  thread-based tests are kept in their own phase because they need
  synchronization (barriers, channels) and are the most likely to be flaky.
- *Are the phase order and dependencies clear, and can phases be implemented
  and verified independently where practical?* Yes. Each phase has its own
  single-threaded tests, except P-004, which only adds tests over the API of
  P-001 to P-003. P-005 builds on the poisoning behavior of P-003 and has its
  own tests. `FL-RS-006` and the doc-comment part of `FL-FR-016` are
  documentation requirements, verified during P-008 by reviewing the doc
  comments.
- *What risks, unknowns, or decisions could change the phase plan?* (1)
  Operating-system file locks are no longer a risk for this plan: since
  revision 0.2 they belong to the future `FileAtlas`, and `FileLock` stays
  independent of it. (2) Poison recovery was decided in revision 0.3 and
  planned as P-005; the risk left is that `Drop` during unwinding must stay
  panic-free (`FL-RS-007`), otherwise the process aborts.
  (3) Timing-dependent tests could be flaky; they must use barriers and
  `try_*` checks rather than sleeps where possible.
- *Revision 0.5 planning:* the change is three small trait impls, so P-007
  is short; it is split from P-008 so the unchanged T-001 to T-030 prove the
  impls are additive before the integration tests exist. P-008 depends on
  P-007 only. Risks: (1) the inference break described in
  [Compatibility](#compatibility), which no code in this workspace hits;
  (2) a view that holds a write guard and panics must still produce a
  `PoisonReport`, which T-034 checks; (3) long-lived views holding a read
  guard delay writers, which is the same fairness policy as any read guard
  and is documented, not prevented.

**Implementation reflection (update as work progresses):**

- Not started.
- Revision 0.3 (before implementation): phase P-005 was added for poison
  tracking and recovery, after the user chose explicit recovery with a
  snapshot report. Coverage and Documentation were renumbered from P-005 and
  P-006 to P-006 and P-007; none of them had been started.
- Implementation (2026-11-11): P-001, P-002, P-003, and P-005 were written
  together in `src/file/file_lock.rs`, because the guards, the poisoning
  mapping, and the snapshot share the same few types; they were then
  verified phase by phase with their tests. All completion criteria were met.
- The write guard follows the std poisoning rule exactly: it stores a
  `PoisonReport` (and skips the generation increment) only when its thread
  started panicking *after* the guard was acquired, because only then does
  the inner std guard poison the lock. A write guard acquired and dropped
  during the same unwinding (for example inside another `Drop`) does not
  poison, so it counts as a normal release. This refines the wording of
  `FL-FR-013` and `FL-FR-015` without changing their intent.
- The snapshot calls `FileMetadata::file_len` and `FileMetadata::element_count`,
  which each query the file length, so write-guard acquisition costs two
  `metadata()` system calls, not one. The Performance section was corrected
  during P-007; `FL-DS-002` is unchanged (only the file length is queried).
- Panic-free `Drop` (`FL-RS-007`): the only call that could in theory panic
  is `std::thread::current()` after the thread's local storage was destroyed,
  which cannot happen while a write guard is in use in normal code.
- Coverage found that T-014 did not check `get_mut` on a poisoned lock,
  although `FL-FR-012` requires it; the check was added to T-014.
- Revision 0.4: phase P-006 was added for buffered reader and writer
  integration tests, at the user's request. Coverage and Documentation, which
  were already done, were renumbered to P-007 and P-008, unchecked, and
  redone after P-006. The buffered API worked as-is through `Deref` /
  `DerefMut`, so `buffered_reader.rs` and `buffered_writer.rs` were not
  changed. `file_lock.rs` was not changed either, so its coverage is the same.
- Revision 0.5 (before implementation, planned 2026-11-11 at `aa9eaf4`):
  phases P-007 (`Borrow` impls) and P-008 (`StructFile` integration tests)
  were added at the user's request, to resolve the deferred item of
  `struct_file.md` revision 0.3. Coverage and Documentation, which were done,
  were renumbered from P-007 and P-008 to P-009 and P-010, unchecked, and
  must be redone after P-008.
- Revision 0.5 implementation (2026-11-11): the plan held with no
  deviation. P-007: each impl body is `self`, which deref-coerces to the
  metadata, so `borrow()` and `Deref` cannot disagree (`FL-RS-008`).
  T-001 to T-030 passed unchanged, confirming the change is additive, and
  `file_lock.rs` names no `StructFile` or `SerializedStruct`. P-008: T-031 to
  T-034 passed on 10 repeated runs, and `src/file/struct_file.rs` was not
  changed. T-034 confirms that a write view holding its guard behaves like a
  plain write guard on panic: the view is dropped during unwinding, the guard
  stores the `PoisonReport` with the rows the view wrote. P-009: coverage
  rerun; P-010: doc comments added on the three impls, and
  `doc/requirements/file/struct_file.md` updated to revision 0.4.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `FL-FR-001`, `FL-FR-011` | Build a `FileLock` from a `FileMetadata` with `new` and with `From`, then call `into_inner`. | The returned `FileMetadata` has the original `element_size`, `data_offset`, and `element_count`. |
| T-002 | `FL-FR-002` | `open_or_create` on a new temporary path, and with `element_size == 0`. | The first call returns `Ok` with zero rows; the second returns `Err` with the `FileMetadata` message. |
| T-003 | `FL-FR-010` | Call `get_mut` on an owned `FileLock` and append a row. | The call returns `Ok`, and `element_count` is 1 after the append. |
| T-004 | `FL-RS-001` | Compile-time assertion that `FileLock: Send + Sync`. | The test compiles. |
| T-005 | `FL-DS-001`, `FL-DS-002` | Acquire and drop read and write guards without calling any operation, then check the file length and element count. | The file length and element count are unchanged, and no other file is created in the directory. |
| T-006 | `FL-FR-003`, `FL-FR-005` | Hold two read guards at the same time on one thread and read the same row through both. | Both guards are obtained and return the same row bytes. |
| T-007 | `FL-FR-004`, `FL-FR-006` | Take a write guard, write and append rows through `DerefMut`, drop it. | The operations return `Ok` and the row count matches. |
| T-008 | `FL-FR-009` | Write rows through a write guard, drop it, then read them through a new read guard. | The read guard sees the written values and the new element count. |
| T-009 | `FL-RS-004` | Iterate `rows()` through a read guard. | All rows are returned in order. |
| T-010 | `FL-FR-007` | Drop a guard while unwinding from a panic inside `std::panic::catch_unwind` with a read guard, then acquire a write guard. | The write guard is acquired after the unwind (read guards do not poison). |
| T-011 | `FL-FR-008` | While a read guard is held: `try_read` returns `Some`, `try_write` returns `None`. | Results match. |
| T-012 | `FL-FR-008` | While a write guard is held: `try_read` and `try_write` both return `None`; after dropping it, both return `Some`. | Results match. |
| T-013 | `FL-FR-007`, `FL-FR-008` | Drop all read guards and check `try_write`. | `try_write` returns `Some` once the last read guard is dropped and `None` before. |
| T-014 | `FL-FR-012` | Panic in a thread while it holds a write guard, then call `read`, `write`, `try_read`, `try_write`, and `into_inner`. | Every call returns `Err` mentioning poisoning; none panics. |
| T-015 | `FL-FR-003`, `FL-RS-001` | Spawn N reader threads on `Arc<FileLock>` that meet at a `Barrier` while each holds a read guard. | All threads pass the barrier, proving the read guards are held at the same time. |
| T-016 | `FL-FR-004`, `FL-FR-007` | A thread holds a write guard; another thread calls `read` and reports through a channel when it gets the guard. | No report arrives before the writer drops its guard; the report arrives after. |
| T-017 | `FL-FR-004`, `FL-FR-009` | Several writer threads each append M rows through `write`, then the main thread reads all rows. | The final element count is writers × M, and every row is complete and not corrupted. |
| T-018 | `FL-DS-001` | Write rows through a `FileLock`, call `into_inner`, drop it, reopen the path with plain `FileMetadata::open_or_create`. | The reopened file has the same rows and metadata bytes. |
| T-019 | `FL-FR-013` | In a named thread, take a write guard on a file with N rows, append one row, then panic. Call `recover`. | It returns `Some(report)` with `before.element_count == N`, `after == Some(..)` with `element_count == N + 1` and a larger `file_len`, and the thread name. |
| T-020 | `FL-FR-012`, `FL-FR-014` | Poison the lock, call `recover`, then `read`, `write`, `into_inner`, and `recover` again. | After the first `recover`, every call succeeds, and the second `recover` returns `Ok(None)`. |
| T-021 | `FL-FR-014` | Call `recover` on a lock that was never poisoned, and while a read guard is held on another thread that is then released. | It returns `Ok(None)` after the reader releases, and the file and `generation` are unchanged. |
| T-022 | `FL-FR-015` | Drop two write guards normally, two read guards, use `get_mut`, then poison with a third write guard. Check `generation` after each step, including on the poisoned lock. | `generation` is 2 at the end, unchanged by read guards, `get_mut`, and the panicking guard, and readable while poisoned. |
| T-023 | `FL-FR-016` | Take a write guard, overwrite an existing row in place with `write_row`, then panic. Call `recover`. | The report has `before == after`, showing that in-place overwrites are not detected. |
| T-024 | `FL-FR-017` | Build two `BufferedReader`s from two read guards held at the same time; read single rows and iterate `rows()`. | Both readers return the expected rows; `len()` matches the row count. |
| T-025 | `FL-FR-018` | Build a `BufferedWriter` from a write guard, overwrite and append rows, `flush()`, drop the writer, then use the guard again. | The guard sees 7 rows after the writer is dropped; a later read guard sees the written values; `generation` is 1. |
| T-026 | `FL-FR-019` | Write rows through a `BufferedWriter` built from a write guard without calling `flush()`, and let the scope end. | A read guard taken afterwards sees all 5 rows, read through a `BufferedReader`. |
| T-027 | `FL-FR-017`, `FL-RS-001` | Four threads each build a `BufferedReader` on their own read guard, meet at a `Barrier`, then read every row. | All threads pass the barrier and compute the same row sum. |
| T-028 | `FL-FR-018`, `FL-FR-019` | A thread holds a `BufferedWriter` with one pending row; another thread tries to read the row through a `BufferedReader`. | `try_read` returns `None` and the reader gets nothing within 200 ms while the writer is alive; after release, the reader sees the flushed row. |
| T-029 | `FL-FR-013`, `FL-FR-019` | Write two rows through a `BufferedWriter` built from the guard, drop it, then panic while still holding the guard. | The report has `before.element_count == 2` and `after.element_count == 4`. |
| T-030 | `FL-FR-019` | Panic while a `BufferedWriter` built from the guard still has a pending row. | Unwinding flushes the row before the guard records `after`: the report shows 0 then 1 rows, and the row is readable after `recover`. |
| T-031 | `FL-FR-020`, `FL-FR-021`, `FL-RS-008` | Call `Borrow::borrow` on a read guard and on a write guard, and `BorrowMut::borrow_mut` on a write guard to append a row; compare with `Deref`; drop the write guard. | `borrow()` and `borrow_mut()` point to the same `FileMetadata` as `Deref` (`std::ptr::eq`); the appended row is visible to a later read guard and `generation` is 1. |
| T-032 | `FL-FR-020`, `FL-FR-022` | Build two `StructFile<Record, FileReadGuard<'_>>` views from two `read()` calls; read through both; check `try_write` while they live and after they are dropped. | Both views return the same records; `try_write` returns `None` while a view is alive and `Some` after both are dropped. |
| T-033 | `FL-FR-021`, `FL-FR-022` | Build `StructFile<Record, FileWriteGuard<'_>>` from `write()`, append and insert records, call `into_inner` to get the guard back, check `element_count` through it, drop it, then read through a read-guard view. | The returned guard sees the new count; `generation` is 1 after it is dropped; the read view returns the records in order. |
| T-034 | `FL-FR-013`, `FL-FR-022` | In a thread named `poisoner`, build a write view from a guard on a file with N rows, append two records, then panic while the view is alive. Call `recover`. | `recover` returns a report with `before.element_count == N` and `after.element_count == N + 2`; after `recover`, a read view sees N + 2 records. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `rust_serial_file/tests/file_lock.rs` | `new_and_into_inner_keep_metadata` |
| T-002 | `rust_serial_file/tests/file_lock.rs` | `open_or_create_forwards_metadata_errors` |
| T-003 | `rust_serial_file/tests/file_lock.rs` | `get_mut_gives_unlocked_mutable_access` |
| T-004 | `rust_serial_file/tests/file_lock.rs` | `file_lock_is_send_and_sync` |
| T-005 | `rust_serial_file/tests/file_lock.rs` | `locking_performs_no_file_io` |
| T-006 | `rust_serial_file/tests/file_lock.rs` | `multiple_read_guards_coexist` |
| T-007 | `rust_serial_file/tests/file_lock.rs` | `write_guard_allows_mutation` |
| T-008 | `rust_serial_file/tests/file_lock.rs` | `writes_visible_after_write_guard_drop` |
| T-009 | `rust_serial_file/tests/file_lock.rs` | `read_guard_supports_row_iteration` |
| T-010 | `rust_serial_file/tests/file_lock.rs` | `read_guard_released_on_unwind` |
| T-011 | `rust_serial_file/tests/file_lock.rs` | `try_locks_while_reading` |
| T-012 | `rust_serial_file/tests/file_lock.rs` | `try_locks_while_writing` |
| T-013 | `rust_serial_file/tests/file_lock.rs` | `try_write_after_last_read_guard_drop` |
| T-014 | `rust_serial_file/tests/file_lock.rs` | `poisoned_lock_returns_errors` |
| T-015 | `rust_serial_file/tests/file_lock.rs` | `concurrent_readers_across_threads` |
| T-016 | `rust_serial_file/tests/file_lock.rs` | `writer_blocks_readers_until_released` |
| T-017 | `rust_serial_file/tests/file_lock.rs` | `concurrent_writers_are_serialized` |
| T-018 | `rust_serial_file/tests/file_lock.rs` | `file_written_through_lock_reopens_as_metadata` |
| T-019 | `rust_serial_file/tests/file_lock.rs` | `recover_reports_snapshot_of_interrupted_write` |
| T-020 | `rust_serial_file/tests/file_lock.rs` | `recover_clears_poison` |
| T-021 | `rust_serial_file/tests/file_lock.rs` | `recover_on_healthy_lock_is_noop` |
| T-022 | `rust_serial_file/tests/file_lock.rs` | `generation_counts_clean_write_releases` |
| T-023 | `rust_serial_file/tests/file_lock.rs` | `report_does_not_detect_in_place_overwrite` |
| T-024 | `rust_serial_file/tests/file_lock.rs` | `buffered_reader_borrows_through_read_guard` |
| T-025 | `rust_serial_file/tests/file_lock.rs` | `buffered_writer_borrows_through_write_guard` |
| T-026 | `rust_serial_file/tests/file_lock.rs` | `buffered_writer_drop_flushes_before_write_lock_release` |
| T-027 | `rust_serial_file/tests/file_lock.rs` | `buffered_readers_on_concurrent_threads` |
| T-028 | `rust_serial_file/tests/file_lock.rs` | `buffered_writer_blocks_buffered_readers_until_released` |
| T-029 | `rust_serial_file/tests/file_lock.rs` | `panic_with_buffered_writer_reports_flushed_rows` |
| T-030 | `rust_serial_file/tests/file_lock.rs` | `panic_while_buffered_writer_alive_flushes_during_unwind` |
| T-031 | `rust_serial_file/tests/file_lock.rs` | `guard_borrow_matches_deref` |
| T-032 | `rust_serial_file/tests/file_lock.rs` | `struct_file_views_hold_read_guards` |
| T-033 | `rust_serial_file/tests/file_lock.rs` | `struct_file_view_holds_write_guard_until_dropped` |
| T-034 | `rust_serial_file/tests/file_lock.rs` | `panic_with_struct_file_write_view_poisons_and_reports` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-002 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-003 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-004 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-005 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-006 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-007 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-008 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-009 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-010 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-011 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-012 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-013 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-014 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-015 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-016 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-017 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-018 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-019 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-020 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-021 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-022 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-023 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-024 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-025 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-026 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-027 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-028 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-029 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-030 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-031 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-032 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-033 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |
| T-034 | 2026-11-11 | `aa9eaf4+dirty` | ✅ Success |

#### Notes

- Environment: Windows 10, `cargo 1.95.0`, run from `rust_serial_file/`.
  Command: `cargo test --test file_lock`. The full crate suite
  (`cargo test`, 8 test binaries) also passes, as do `cargo fmt --check` and
  `cargo clippy --all-targets` with no warnings.
- P-004 reliability criterion: `cargo test --test file_lock` was run 10
  times in a row; all 23 tests passed every time (about 0.2 s per run).
- T-014, T-019, T-020, T-022, and T-023 panic on purpose in a thread named
  `poisoner`; the panic messages printed on stderr are expected.
- T-015 has no timeout: if read guards were exclusive it would hang instead
  of failing. T-016 uses a 200 ms window to check that the reader is blocked.
- T-014 was extended during the first coverage run to also check `get_mut`
  on a poisoned lock.
- Revision 0.4 run: 30 tests, 10 repeated runs, all passed; the full crate
  suite also passes. T-030 panics on purpose in an unnamed thread; its panic
  message is expected.
- T-024 to T-030 needed no change to `BufferedReader` or `BufferedWriter`:
  `&guard` and `&mut guard` coerce to `&FileMetadata` and
  `&mut FileMetadata`.
- Revision 0.5 run (2026-11-11, `aa9eaf4+dirty`): `cargo test --test
  file_lock` was run 10 times in a row; all 34 tests passed every time
  (about 0.23 s per run). The full crate suite (`cargo test`, 179 tests with
  doc tests) passes, as do `cargo fmt --check` and
  `cargo clippy --all-targets` with no warnings. T-001 to T-030 passed with
  no change to their code after P-007.
- T-034 panics on purpose in the `poisoner` thread; its panic message is
  expected.
- The tests use a `Value { value: u32 }` record derived with
  `SerializedStruct`, matching the 4-byte little-endian rows the other tests
  write, so T-032 to T-034 read the rows written by `lock_with_rows`.

#### Warning notes

- None.

### Acceptance criteria

1. The specified public behavior is implemented and documented.
2. Relevant boundary and failure cases are tested.
3. Existing behavior and compatibility requirements are preserved.
4. The component passes the applicable formatting, lint, build, and test checks.

## Code coverage

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** `src/file/file_lock.rs` has 100% function and line
  coverage and 98.03% region coverage; see the table below. Revision 0.5
  touched only this production file (plus the test file
  `tests/file_lock.rs`, which is not reported). Before revision 0.5 it
  measured 100.00% (18/18) functions, 100.00% (100/100) lines, and 97.90%
  (140/143) regions; `src/file/mod.rs`, touched in revision 0.1, has no
  executable code.

### Current coverage results

- **Tool and command:** `cargo-llvm-cov 0.9.1` with `llvm-tools-preview`, on
  the stable toolchain, run from `rust_serial_file/`: `cargo llvm-cov clean`,
  then `cargo llvm-cov --all-targets`, then
  `cargo llvm-cov report --show-missing-lines`. Branch coverage is `N/A`:
  `--branch` needs a nightly compiler.
- **Date of run:** 2026-11-11
- **Git hash:** `aa9eaf4+dirty`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `rust_serial_file/src/file/file_lock.rs` | 100.00% (21/21) | 100.00% (109/109) | 98.03% (149/152) | N/A |

### Coverage notes

- **`rust_serial_file/src/file/file_lock.rs`:** the three new `Borrow` /
  `BorrowMut` functions are fully covered. The same 3 regions as before are
  not covered, all error paths of the write-guard snapshot: the `?` after
  `metadata.file_len()` (line 60) and after `metadata.element_count()`
  (line 61) in `FileSnapshot::capture`, and the `?` that passes that error up
  in `FileWriteGuard::new` (line 300). They run only if `File::metadata()`
  fails on a file that is already open, which cannot be triggered portably
  from a test without fault injection. No plan to cover them now; a
  fault-injection seam in `FileMetadata` would be needed.

## Out of scope

- **Operating-system file locks (resolved in revision 0.2):** `FileLock` does
  not take or check any OS file lock. `FileAtlas`, specified in
  `doc/requirements/file/file_atlas.md`, owns the files it opens and takes an
  exclusive OS lock (`File::try_lock`, fail fast) on each when it opens it.
  `FileLock` has no knowledge of `FileAtlas`. A `FileLock` created without
  the atlas still takes no OS lock, so its caller must ensure on their own
  that no other process or handle accesses the file (`FL-RS-006`).
- **Poison recovery (resolved in revision 0.3):** poisoning is reported as an
  error until the caller calls `recover`, which returns a `PoisonReport` and
  clears the poison (`FL-FR-012` to `FL-FR-016`). Still out of scope:
  - automatic resynchronization of the cached `FileMetadata` counts during
    `recover` (this would need a new public `FileMetadata::refresh()`);
  - capturing the panic message (only possible with an application-level
    panic hook);
  - an undo journal, row checksums, or any other way to detect or undo rows
    overwritten in place; this belongs to a future transaction layer, possibly
    in `FileAtlas`.
- **Mid-operation I/O errors in `FileMetadata` (open issue, to be resolved by
  the module owner, Olographik, in the `FileMetadata` requirements or a future
  transaction layer):** `insert_row`, `truncate_rows_front`, and
  `update_raw_metadata` change the file in several steps. An I/O error between
  steps returns `Err` but can leave the file partially changed (for example
  rows shifted but `data_offset` not updated). It does not poison the lock, so
  `PoisonReport` does not cover it.
- Upgradable or downgradable guards (read to write, write to read).
- Timeout-based acquisition (`read_timeout`, `write_timeout`).
- A lock-aware `StructFile`, buffered reader, or buffered writer built on top
  of `FileLock`; these will have their own requirement documents. Since
  revision 0.5 a plain `StructFile` can hold a guard (`FL-FR-022`), which
  covers the typed-view case without a lock-aware type.
- Owned guards that do not borrow the `FileLock` (for example over
  `Arc<FileLock>`), so a view can outlive a borrow of its lock or of a future
  `FileAtlas`. Owner: the user, when the atlas is specified.
- `Borrow` impls for `BufferedReader` or `BufferedWriter`: they already take
  `&FileMetadata` / `&mut FileMetadata`, which a guard reborrow provides.
- Async (`tokio`) locking.
- Fairness or writer-priority guarantees beyond those of `std::sync::RwLock`.
