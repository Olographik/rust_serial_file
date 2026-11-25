# File Atlas Requirements

- **Date:** 2026-11-11
- **Author:** `Olographik`
- **Revision:** 0.2
- **Short description:** Define a thread-safe `FileAtlas<K>` that opens row files under generic keys, holds each one's operating-system lock, and hands out shared `FileLock` handles, closure-scoped `FileMetadata` access, and typed `StructFile` views.

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

An application that works with several row files needs one place that owns
the opened files, stops another process from using them at the same time, and
lets any thread reach a file by name. `FileLock`
(`doc/requirements/file/file_lock.md`) already coordinates threads around one
`FileMetadata`, but it is in-process only and deliberately leaves
operating-system (OS) locking to a future owner of the opened files. This
document specifies that owner: `FileAtlas<K>`.

`FileAtlas<K>` is a map from a generic key `K` to an opened file. Each value
is a `FileLock` stored in an `Arc`, so it can be shared with any thread. The
atlas:

- **opens** a file by path under a key, takes an exclusive OS lock on it
  (failing at once if another process or handle holds it), and stores it;
- gives out a **handle** (`Arc<FileLock>`) for a key. The caller locks the
  handle for reading or writing, and can build a `StructFile` that holds the
  guard: `StructFile::<Rec, _>::new(handle.read()?)`;
- runs **closures** with scoped access for one-shot work: read-only or
  writable `FileMetadata`, or a read-only or writable `StructFile<T, _>`;
- **iterates** over a snapshot of its entries, as `(K, Arc<FileLock>)`
  pairs, so `for (key, handle) in &atlas` works without holding the map lock
  while the loop runs;
- **removes** a file only when no other handle is in use, and returns its
  `FileLock` to the caller. The OS lock is released when that `FileLock` (and
  its `FileMetadata`) is dropped.

The atlas itself is `Send + Sync` when `K` is, so it can be shared with
`Arc<FileAtlas<K>>`; the map sits behind its own `RwLock`, separate from each
file's `FileLock`.

**Why handles and closures rather than returned guards:** the `FileLock`
guards borrow their `FileLock`. A guard returned by an atlas method would
borrow a value inside the atlas's map and keep the map locked, which would
block every other thread from opening or removing files. Returning an
`Arc<FileLock>` handle avoids this, and the caller's guard borrows the
caller's own handle. Guards that own their lock (so the atlas could return a
ready `StructFile` in one call) would need either `unsafe` code in `FileLock`
or a different lock crate; they are deferred (see
[Out of scope](#out-of-scope)). No `FileLock` change is needed for this
revision.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-11 | `Olographik` | Iteration, before implementation. Added `FA-FR-012` (`iter` snapshot), `FA-FR-013` (`IntoIterator for &FileAtlas`), `FA-FR-014` (`IntoIterator for FileAtlas`), `FA-FR-015` (snapshot handles count as in use), `FA-FR-016` (no `Index`), and `FA-RS-008` (iterator types and traits). Added phase P-005 (iteration); Coverage and Documentation renumbered to P-006 and P-007. Added tests T-020 to T-023. Updated Overview, Scope, Public API, Performance, planning reflection, and Out of scope (`Index` and `SliceIndex` cannot be implemented, with reasons). |

## Goals and scope

### Goals

- Own every opened file of an application in one shareable structure,
  reachable by a key of the caller's type.
- Make sure no other process (and no other handle in this process) uses a
  file while the atlas holds it, using an exclusive OS lock taken on open.
- Give threads concurrent read access and exclusive write access to each
  file through the existing `FileLock`, without blocking access to other
  files.
- Give typed access through `StructFile` for both reading and writing.
- Report every failure (key conflicts, missing keys, files in use, OS lock
  conflicts, I/O errors) as an error; never panic on a lock state.

### Scope

- New module `rust_serial_file/src/file/file_atlas.rs`, exported from
  `rust_serial_file/src/file/mod.rs` as `pub mod file_atlas;`.
- Types `FileAtlas<K>` and `FileAtlasError`.
- Opening (`open_or_create`, `open_struct`), lookup (`get`, `contains`,
  `len`, `is_empty`, `keys`), removal (`remove`), closure access
  (`with_read`, `with_write`, `with_struct_read`, `with_struct_write`), and
  iteration (`iter`, `IntoIterator` for `&FileAtlas<K>` and `FileAtlas<K>`,
  with the iterator types `FileAtlasIter<K>` and `FileAtlasIntoIter<K>`).
- `FileLock`, `FileMetadata`, `StructFile`, `BufferedReader`, and
  `BufferedWriter` are used as they are and are not changed.
- `doc/requirements/file/file_lock.md` is updated in the Documentation phase
  only to point to this document from its OS-lock Out of scope item.

## Assumptions and dependencies

- Uses `std::fs::File::try_lock` for the exclusive OS lock. It was added to
  std in Rust 1.89. The local toolchain is Rust 1.95. The crate declares
  edition 2024 and no MSRV; this component raises the real minimum to 1.89
  (see [Out of scope](#out-of-scope) for declaring it in `Cargo.toml`).
- OS lock semantics are those of `File::try_lock`: an advisory lock on Unix
  (`flock`), a mandatory byte-range lock on Windows (`LockFileEx`). On Unix
  it only stops other processes and handles that also take a lock; on both,
  two handles on the same file in the same process conflict, so opening the
  same path under two keys fails. On a platform without file locking,
  `try_lock` returns an error and opening fails.
- The lock is held by the `File` inside the `FileMetadata`, and is released
  when that `File` is closed, that is when the `FileMetadata` (or the
  `FileLock` owning it) is dropped. `FileMetadata` does not expose its `File`,
  and the atlas does not unlock explicitly.
- Files are opened the same way as `FileMetadata::open_or_create`
  (read, write, create, no truncate). The atlas opens the `File` itself so it
  can lock it before building the `FileMetadata` with `FileMetadata::new`.
- `K: Hash + Eq` (stored in a `HashMap`). Lookups accept any borrowed form
  `Q` of the key (`K: Borrow<Q>`), as `HashMap::get` does, so a
  `FileAtlas<String>` can be queried with a `&str`.
- Dependencies: the standard library (`HashMap`, `RwLock`, `Arc`, `File`)
  and `anyhow`, already used by the crate; `tempfile` for tests. No new
  crate.
- Path equality is not checked by the atlas: two different paths that point
  to the same file (a symlink, a relative and an absolute path) are caught by
  the OS lock conflict, not by comparing paths.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `FA-FR-001` | `FileAtlas::new() -> FileAtlas<K>` creates an empty atlas. `FileAtlas<K>` implements `Default` with the same result. | 0.1 |
| `FA-FR-002` | `open_or_create(&self, key: K, path, element_size: u64, data_offset: u64) -> Result<Arc<FileLock>>` opens or creates the file at `path` (read, write, create, no truncate), takes an exclusive OS lock with `File::try_lock`, builds a `FileMetadata` with `FileMetadata::new`, wraps it in a `FileLock`, stores it under `key`, and returns a handle to it. | 0.1 |
| `FA-FR-003` | If `key` is already present, `open_or_create` returns `FileAtlasError::KeyAlreadyPresent` without opening `path` or changing the existing entry. Entries are never replaced implicitly. | 0.1 |
| `FA-FR-004` | If the OS lock cannot be taken at once because another process or another handle holds it, `open_or_create` returns `FileAtlasError::FileLocked` without waiting and without adding an entry; the file is closed again. Other lock failures (for example an unsupported platform) are returned as errors with the path as context. | 0.1 |
| `FA-FR-005` | `open_struct::<T: SerializedStruct>(&self, key, path, data_offset) -> Result<Arc<FileLock>>` behaves as `open_or_create` with `element_size = T::SIZE`, and fails if `T::SIZE` is 0. | 0.1 |
| `FA-FR-006` | `get(&self, key: &Q) -> Option<Arc<FileLock>>` returns a new handle to the entry, or `None`. Every handle to an entry points to the same `FileLock` (`Arc::ptr_eq`). `contains(&self, key: &Q) -> bool`, `len(&self) -> usize`, and `is_empty(&self) -> bool` report the map content. `keys(&self) -> Vec<K>` (with `K: Clone`) returns a snapshot of the keys in unspecified order. | 0.1 |
| `FA-FR-007` | `remove(&self, key: &Q) -> Result<FileLock>` removes the entry and returns its owned `FileLock` only when the atlas holds the last handle to it. If any other handle exists, it returns `FileAtlasError::InUse` and leaves the entry in the atlas. If the key is missing, it returns `FileAtlasError::KeyNotFound`. The returned `FileLock` keeps the OS lock until it is dropped, and can be inspected with `recover` or turned into its `FileMetadata` with `into_inner`, even if it is poisoned. | 0.1 |
| `FA-FR-008` | `with_read(&self, key, f: impl FnOnce(&FileMetadata) -> Result<R>) -> Result<R>` takes a read guard on the entry, calls `f`, releases the guard, and returns `f`'s result. `with_write(&self, key, f: impl FnOnce(&mut FileMetadata) -> Result<R>) -> Result<R>` does the same with a write guard. A missing key returns `FileAtlasError::KeyNotFound`; a poisoned entry returns the `FileLock` poisoning error; an error from `f` is returned unchanged. | 0.1 |
| `FA-FR-009` | `with_struct_read::<T, R>(&self, key, f: impl FnOnce(&StructFile<T, FileReadGuard<'_>>) -> Result<R>) -> Result<R>` builds a read-only `StructFile` holding a read guard and calls `f`. `with_struct_write::<T, R>(&self, key, f: impl FnOnce(&mut StructFile<T, FileWriteGuard<'_>>) -> Result<R>) -> Result<R>` does the same with a writable view holding a write guard. If the file's row width is not `T::SIZE`, the `StructFile` construction error is returned and `f` is not called. | 0.1 |
| `FA-FR-010` | While any handle from `get` or `open_*` exists, its entry cannot be removed (`FA-FR-007`). So the atlas never gives up a file that a thread is using, and every live handle points to a file the atlas still owns. | 0.1 |
| `FA-FR-011` | Access to different entries is independent: a write guard (or a closure holding one) on one entry never blocks reads or writes on another entry, nor `open_*`, `get`, `contains`, `len`, `keys`, or `remove` of another entry. | 0.1 |
| `FA-FR-012` | `iter(&self) -> FileAtlasIter<K>` (with `K: Clone`) returns an iterator over a snapshot of the entries, as `(K, Arc<FileLock>)` pairs: the key is cloned and the handle is a new `Arc`. The snapshot is taken under the shared map lock, which is released before `iter` returns. Entries opened or removed after the call do not change the snapshot. The order is unspecified, as for `keys`. | 0.2 |
| `FA-FR-013` | `&FileAtlas<K>` implements `IntoIterator` (with `K: Clone + Hash + Eq`) with `Item = (K, Arc<FileLock>)` and `IntoIter = FileAtlasIter<K>`, returning the same snapshot as `iter`, so `for (key, handle) in &atlas` works. | 0.2 |
| `FA-FR-014` | `FileAtlas<K>` implements `IntoIterator` with `Item = (K, Arc<FileLock>)` and `IntoIter = FileAtlasIntoIter<K>`, consuming the atlas and yielding every entry without cloning keys. A poisoned map lock is recovered as in `FA-RS-003`. Each file and its OS lock are released when the last handle to it is dropped. | 0.2 |
| `FA-FR-015` | Handles held by a snapshot iterator (`FA-FR-012`, `FA-FR-013`) are ordinary handles: while an iterator or a yielded handle is alive, `remove` of that entry returns `FileAtlasError::InUse` (`FA-FR-007`). Dropping the iterator releases the handles it has not yielded yet. Other atlas methods, including `open_*`, `remove` of entries not in the snapshot, and `with_*`, can be called while iterating without deadlock. | 0.2 |
| `FA-FR-016` | `FileAtlas<K>` does not implement `std::ops::Index` or `IndexMut`. Keyed access is `get` (owned handle) and the `with_*` closures. The reason is recorded in [Out of scope](#out-of-scope). | 0.2 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `FA-DS-001` | The atlas adds no on-disk data: no index file, lock file, header, or sidecar. The file layout is exactly the one defined by `FileMetadata`, and a file written through the atlas can be reopened with a plain `FileMetadata` after the atlas released it. | 0.1 |
| `FA-DS-002` | Opening an existing file does not change its contents or length; creating a missing file creates an empty file. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `FA-RS-001` | `FileAtlas<K>` is `Send + Sync` when `K: Send + Sync`, derived automatically (no `unsafe impl`); checked by a compile-time assertion in the tests. | 0.1 |
| `FA-RS-002` | The map is stored as `RwLock<HashMap<K, Arc<FileLock>>>`. The map lock is held only for the map operation itself; it is released before any `FileLock` guard is taken and before any user closure runs. A closure may therefore call any atlas method, including on the same key, without deadlocking on the map lock. (Taking a second write guard on the same entry from inside a `with_write` closure still deadlocks, as for any `FileLock`, and is documented.) | 0.1 |
| `FA-RS-003` | The atlas recovers from a poisoned map lock instead of reporting it: the only code run under the map lock is `HashMap` operations and `K`'s `Hash`/`Eq`, and the entries are `Arc` handles, so a panic in `K`'s `Hash` or `Eq` cannot leave a `FileLock` half-updated. After such a panic, later calls keep working. Poisoning of an entry's `FileLock` is still reported (`FA-FR-008`). | 0.1 |
| `FA-RS-004` | Lookups (`get`, `contains`, `remove`, `with_*`) are generic over `Q: Hash + Eq + ?Sized` with `K: Borrow<Q>`. | 0.1 |
| `FA-RS-005` | `FileAtlasError` is a public enum with the variants `KeyAlreadyPresent`, `KeyNotFound`, `InUse`, and `FileLocked`, implementing `Debug`, `Display`, `PartialEq`, `Eq`, and `std::error::Error`. It is returned inside `anyhow::Error`, so callers distinguish cases with `downcast_ref::<FileAtlasError>()`. Its variants carry no key, path, or file data. | 0.1 |
| `FA-RS-006` | The module contains no `unsafe` code. It does not change `FileLock`, `FileMetadata`, or `StructFile`. | 0.1 |
| `FA-RS-007` | Doc comments on `FileAtlas`, `open_or_create`, and `remove` state the OS lock behavior: exclusive, taken on open, fail-fast, advisory on Unix, released only when the `FileLock` returned by `remove` (or the atlas) is dropped. | 0.1 |
| `FA-RS-008` | `FileAtlasIter<K>` and `FileAtlasIntoIter<K>` are public newtypes over the standard iterators they use (`std::vec::IntoIter` and `std::collections::hash_map::IntoIter`), so those types are not part of the public API. Both implement `Iterator`, `ExactSizeIterator` (exact `len`), and `FusedIterator`, and are `Send + Sync` when `K` is. | 0.2 |

## Public API

Module `rust_serial_file::file::file_atlas`:

```rust
pub struct FileAtlas<K> { /* RwLock<HashMap<K, Arc<FileLock>>> */ }

impl<K: Hash + Eq> FileAtlas<K> {
    pub fn new() -> Self;

    pub fn open_or_create(
        &self,
        key: K,
        path: impl AsRef<Path>,
        element_size: u64,
        data_offset: u64,
    ) -> Result<Arc<FileLock>>;
    pub fn open_struct<T: SerializedStruct>(
        &self,
        key: K,
        path: impl AsRef<Path>,
        data_offset: u64,
    ) -> Result<Arc<FileLock>>;

    pub fn get<Q>(&self, key: &Q) -> Option<Arc<FileLock>>
    where K: Borrow<Q>, Q: Hash + Eq + ?Sized;
    pub fn contains<Q>(&self, key: &Q) -> bool /* same bounds */;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn keys(&self) -> Vec<K> where K: Clone;
    pub fn iter(&self) -> FileAtlasIter<K> where K: Clone;

    pub fn remove<Q>(&self, key: &Q) -> Result<FileLock> /* same bounds */;

    pub fn with_read<Q, R>(&self, key: &Q, f: impl FnOnce(&FileMetadata) -> Result<R>) -> Result<R>;
    pub fn with_write<Q, R>(&self, key: &Q, f: impl FnOnce(&mut FileMetadata) -> Result<R>) -> Result<R>;
    pub fn with_struct_read<T: SerializedStruct, Q, R>(
        &self,
        key: &Q,
        f: impl FnOnce(&StructFile<T, FileReadGuard<'_>>) -> Result<R>,
    ) -> Result<R>;
    pub fn with_struct_write<T: SerializedStruct, Q, R>(
        &self,
        key: &Q,
        f: impl FnOnce(&mut StructFile<T, FileWriteGuard<'_>>) -> Result<R>,
    ) -> Result<R>;
}

impl<K: Hash + Eq> Default for FileAtlas<K>;

impl<'a, K: Clone + Hash + Eq> IntoIterator for &'a FileAtlas<K> {
    type Item = (K, Arc<FileLock>);
    type IntoIter = FileAtlasIter<K>;
}
impl<K: Hash + Eq> IntoIterator for FileAtlas<K> {
    type Item = (K, Arc<FileLock>);
    type IntoIter = FileAtlasIntoIter<K>;
}

pub struct FileAtlasIter<K> { /* std::vec::IntoIter<(K, Arc<FileLock>)> */ }
pub struct FileAtlasIntoIter<K> { /* hash_map::IntoIter<K, Arc<FileLock>> */ }
// Both: Iterator<Item = (K, Arc<FileLock>)> + ExactSizeIterator + FusedIterator

#[derive(Debug, PartialEq, Eq)]
pub enum FileAtlasError { KeyAlreadyPresent, KeyNotFound, InUse, FileLocked }
impl Display for FileAtlasError;
impl std::error::Error for FileAtlasError;
```

`Result` is `anyhow::Result`. Usage:

```rust
let atlas = Arc::new(FileAtlas::<String>::new());
atlas.open_struct::<Record>("records".to_owned(), path, 0)?;

// Long-lived typed view: keep a handle, lock it, let the view hold the guard.
let handle = atlas.get("records").unwrap();
let reader = StructFile::<Record, _>::new(handle.read()?)?;
let first = reader.serial_get(0)?;
drop(reader);

// One-shot write from any thread.
let shared = Arc::clone(&atlas);
std::thread::spawn(move || {
    shared.with_struct_write::<Record, _, _>("records", |file| file.serial_write(record))
});

// Iterate over a snapshot; the map is not locked during the loop.
for (key, handle) in &*atlas {
    let rows = handle.read()?.element_count()?;
    println!("{key}: {rows} rows");
}

// Release the file and its OS lock.
drop(handle);
let lock = atlas.remove("records")?;   // fails with InUse while handles exist
drop(lock);                            // OS lock released here
```

## Errors and failure behavior

- **Key conflicts:** opening an existing key returns
  `FileAtlasError::KeyAlreadyPresent`; the path is not opened.
- **Missing keys:** `remove` and every `with_*` method return
  `FileAtlasError::KeyNotFound`. `get` returns `None` and `contains` returns
  `false`; they never fail.
- **File in use:** `remove` returns `FileAtlasError::InUse` while any handle
  other than the atlas's own exists. The entry stays. There is no forced
  removal.
- **OS lock conflict:** `open_*` returns `FileAtlasError::FileLocked` when
  `try_lock` reports `WouldBlock`; the opened `File` is dropped and no entry
  is added. Other `try_lock` errors, and open or metadata errors, are
  returned with the path as `anyhow` context, as `FileMetadata::open_or_create`
  does today.
- **Invalid arguments:** `element_size == 0` (or `T::SIZE == 0`) is rejected
  before the file is opened, with the same message as `FileMetadata::new`,
  so no file is created and no entry is added.
- **Entry poisoning:** if a thread panicked while holding an entry's write
  guard (directly or inside `with_write` / `with_struct_write`), later
  `with_*` calls on that entry return the `FileLock` poisoning error. The
  caller recovers through a handle (`atlas.get(key)?.recover()`), or removes
  the entry and handles the returned `FileLock`. `get`, `contains`, and
  `remove` keep working on a poisoned entry.
- **Map poisoning:** recovered silently (`FA-RS-003`); never reported.
- **Closure errors:** returned unchanged. The guard is released before the
  result is returned.
- **Partial operations:** a failed `open_*` leaves the map unchanged. The
  order is: check the key is free, open and lock the file, build the
  `FileMetadata`, then insert under the map write lock, checking the key
  again (another thread may have inserted it meanwhile; then the new file is
  dropped and `KeyAlreadyPresent` is returned). The file may have been
  created on disk even when `open_*` fails, exactly as with
  `FileMetadata::open_or_create`.
- No error is silently converted into a success-shaped result.

## Safety and security

- None: no `unsafe` code is allowed (`FA-RS-006`). `Send` and `Sync` are
  derived automatically.
- Paths come from the caller and are opened as given; the atlas does not
  canonicalize, restrict, or validate them beyond what the OS does. Files
  are created with default permissions, as with
  `FileMetadata::open_or_create`.
- `FileAtlasError` variants carry no keys or paths (`FA-RS-005`); the path
  only appears in the `anyhow` context of open and lock errors, as today.
- Threat assumption: the exclusive OS lock stops cooperating processes on
  Unix (advisory `flock`) and all processes on Windows. It does not protect
  against a process that ignores advisory locks on Unix, or against deletion
  or renaming of the file. This is documented (`FA-RS-007`).

## Compatibility

- No existing public API changes. The only change outside the new file is
  `pub mod file_atlas;` in `src/file/mod.rs`.
- `FileLock` guards, `StructFile` views, and `BufferedReader` /
  `BufferedWriter` are used through handles exactly as they are used today.
- The on-disk format is unchanged (`FA-DS-001`).
- The real minimum Rust version of the crate becomes 1.89 because of
  `File::try_lock`. This is not declared in `Cargo.toml` by this revision
  (see Out of scope).

## Performance and resource use

- `get`, `contains`, `len`, `is_empty`: one shared map-lock acquisition, a
  hash lookup, and (for `get`) one atomic `Arc` increment. No allocation, no
  I/O.
- `keys`: one shared map-lock acquisition and one `Vec` allocation with a
  clone of each key.
- `iter` / `&FileAtlas` iteration: one shared map-lock acquisition and one
  `Vec` allocation holding a cloned key and an `Arc` increment per entry; the
  loop itself takes no map lock. Owned iteration (`FileAtlas` by value)
  takes no lock and does not clone keys.
- `open_*`: one file open, one `try_lock` system call, one file-length
  query (from `FileMetadata::new`), and a short exclusive map-lock
  acquisition for the insert. The file is opened and locked without holding
  the map lock, so a slow open does not block other threads.
- `remove`: a short exclusive map-lock acquisition and an `Arc` strong-count
  check.
- `with_*`: a `get` followed by the `FileLock` guard cost (see
  `file_lock.md`); `with_struct_*` adds the `StructFile::new` cost (one
  element-count query).
- Concurrency: map readers run in parallel; map writers (`open_*`, `remove`)
  are exclusive but short. Each file has its own `FileLock`, so work on
  different files never contends beyond the short map lock (`FA-FR-011`).
  Fairness follows `std::sync::RwLock`.
- One open file descriptor per entry. No limit on the number of entries is
  imposed; the OS limit applies.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Atlas type and opening:** create `src/file/file_atlas.rs`, export it from `src/file/mod.rs`, add `FileAtlasError`, `FileAtlas::new`, `Default`, `open_or_create` (open, `try_lock`, `FileMetadata::new`, checked insert), `open_struct`, `get`, `contains`, `len`, `is_empty`, `keys`, and the map-poison recovery helper. | None | The crate builds; T-001 to T-006, T-009, and T-010 pass. |
| P-002 | **Removal:** implement `remove` with the last-handle check and the returned `FileLock`. | P-001 | T-007, T-008, and T-011 pass. |
| P-003 | **Closure access:** implement `with_read`, `with_write`, `with_struct_read`, and `with_struct_write`, releasing the map lock before taking the entry guard. | P-002 | T-012 to T-015 pass. |
| P-004 | **Concurrency and robustness tests:** add the multi-threaded tests, the map-poison test, the `Send + Sync` assertion, and the on-disk compatibility test. | P-003 | T-016 to T-019 pass on 10 repeated runs; `cargo fmt --check`, `cargo clippy --all-targets`, and `cargo test` pass. |
| P-005 | **Iteration:** add `iter`, the `FileAtlasIter` and `FileAtlasIntoIter` newtypes with `Iterator`, `ExactSizeIterator`, and `FusedIterator`, and the two `IntoIterator` impls. | P-001 (snapshot uses lookups), P-004 (so the full suite is already green) | T-020 to T-023 pass; T-001 to T-019 still pass. |
| P-006 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-005 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-007 | **Documentation:** update this requirements document and the code comments of the touched files (including the OS-lock notes of `FA-RS-007` and the same-entry re-entrancy warning of `FA-RS-002`), and point the OS-lock Out of scope item of `file_lock.md` to this document. | P-006 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments; `file_lock.md` references this document. |

### Current implementation checklist

- [x] P-001 — `Atlas type and opening` — Done
- [x] P-002 — `Removal` — Done
- [x] P-003 — `Closure access` — Done
- [x] P-004 — `Concurrency and robustness tests` — Done
- [x] P-005 — `Iteration` — Done
- [x] P-006 — `Coverage of touched files` — Done
- [x] P-007 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- *Why this breakdown:* the API has three layers that build on each other:
  owning and opening entries, removing them under the in-use rule, and
  scoped access on top of lookups. Each layer has its own tests. The
  thread-based tests are kept in their own phase, as for `FileLock`, because
  they need barriers and are the most likely to be flaky.
- *Order and dependencies:* sequential. P-002 needs entries to exist; P-003
  needs lookups and is easier to test once removal works (T-015 checks that
  a running closure blocks `remove`). P-004 only adds tests.
- *Decisions taken with the user before writing this plan:* handles plus
  closures (no owned guards, no `FileLock` change); exclusive fail-fast OS
  lock; `remove` fails while handles exist; `K: Hash + Eq`.
- *Risks and unknowns:*
  1. OS lock behavior differs by platform. Tests rely only on behavior that
     is the same on Windows and Unix: a second handle in the same process
     fails `try_lock` while the atlas holds the file. If a platform does not
     behave this way, T-004 and T-005 would fail and must be marked
     ⚠️ Mitigate with an explanation.
  2. A race between two threads opening the same new key: handled by the
     second check under the map write lock (`Errors` section), and tested by
     T-017.
  3. Handles make `remove` refuse while any thread holds one. An application
     that keeps long-lived handles cannot remove the file until it drops
     them; this is intended.
  4. `with_*` closures must not take a second guard on the same entry;
     detecting this at runtime is out of scope, as for `FileLock`.
- *Revision 0.2 (before implementation):* iteration was added at the
  user's request as P-005, after the concurrency tests, because it only
  builds on lookups and its tests reuse the same helpers. Coverage and
  Documentation, not started, were renumbered from P-005 and P-006 to P-006
  and P-007. The user asked for `Index`, `SliceIndex`, and iterator traits;
  only the iterator traits can be implemented (see `FA-FR-016` and Out of
  scope). Risk: a snapshot iterator holds handles, so a caller that keeps it
  alive blocks `remove` of those entries; this is documented
  (`FA-FR-015`) rather than avoided, because handing out handles is what
  makes the iteration lock-free.

**Implementation reflection (update as work progresses):**

- Implemented on 2026-11-11 on top of `edf83b6`. P-001, P-002, P-003, and
  P-005 were written together in `src/file/file_atlas.rs`, since they share
  the map helpers, and then verified with their tests; P-004 added the
  thread-based tests. No phase was added, removed, or reordered. The
  requirements document was staged before implementation; implementation
  files were left unstaged, as the user asked.
- **Small deviations, behavior kept within the requirements:**
  - `open_or_create` checks `element_size == 0` before opening the file,
    with the same message as `FileMetadata`, so a zero size (or a zero-sized
    `T` in `open_struct`) does not create the file on disk. T-006 checks
    that the file is not created. `FileMetadata::new` still validates it.
  - T-018 calls `remove`, not `get`, with the panicking key: `get` hashes
    under the map *read* lock, and a panic under a read lock does not poison
    a `std::sync::RwLock`, so it would not exercise `FA-RS-003`. `remove`
    hashes under the write lock.
  - Coverage added two checks: T-002 also opens a path in a missing
    directory (open-error context), and T-022 checks the owned iterator's
    `len()` before looping. The test descriptions were updated.
  - T-023 also checks that the `can_lock` probe used by T-005, T-017, and
    T-022 does not itself keep a file locked.
- **`remove` design:** it uses `HashMap::remove_entry` and
  `Arc::try_unwrap`, and puts the entry back on failure, instead of checking
  `Arc::strong_count` first. While the map write lock is held, no new handle
  can be cloned from the map, so the count cannot grow from one; the
  put-back path only runs when other handles exist.
- **Map poisoning:** the map lock is read and written through helpers that
  recover with `PoisonError::into_inner` (`FA-RS-003`).
- No `FileLock`, `FileMetadata`, or `StructFile` code change was needed
  (`FA-RS-006`). In P-007, two doc comments in `src/file/file_lock.rs`
  that called `FileAtlas` "planned" were changed to link to it, and
  `file_lock.md` was bumped to revision 0.6 to point to this document.
- One test bug was fixed during P-005: T-021 first asserted that every entry
  had 0 rows inside the loop, but the loop itself appends a row to the other
  entry, so the second pass sees 1.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `FA-FR-001`, `FA-FR-006` | Create an atlas with `new` and with `default`; check `len`, `is_empty`, `keys`, `get`, and `contains` on it. | Both are empty: `len() == 0`, `is_empty()`, no keys, `get` returns `None`, `contains` is `false`. |
| T-002 | `FA-FR-002`, `FA-FR-006`, `FA-DS-002` | Open a new path and an existing path holding rows under two keys; call `get` twice on each key. Then open a path inside a missing directory. | Both opens return handles; the new file exists and is empty; the existing file keeps its rows and length; handles for the same key are `Arc::ptr_eq`; `len() == 2`, `keys` contains both. The last open fails with the "failed to open or create" context and adds no entry. |
| T-003 | `FA-FR-003`, `FA-RS-005` | Open a second file under a key already present. | `FileAtlasError::KeyAlreadyPresent`; the second path is not created; the existing entry still returns the original handle. |
| T-004 | `FA-FR-004`, `FA-RS-005` | Open the same path under a second key while the first key holds it. | `FileAtlasError::FileLocked`; no entry is added for the second key. |
| T-005 | `FA-FR-004`, `FA-FR-007` | While the atlas holds a file, open the same path with a plain `File` and call `try_lock`; then `remove` the entry, drop the returned `FileLock`, and try again. | `try_lock` fails with `WouldBlock` while the atlas holds the file and succeeds after the `FileLock` is dropped. |
| T-006 | `FA-FR-005` | `open_struct::<Record>` on a new path, write a record through the handle with a `StructFile`; `open_struct` with a zero-sized type. | The file's element size is `Record::SIZE`; the record round-trips; the zero-sized open fails and adds no entry. |
| T-007 | `FA-FR-007` | `remove` an entry held only by the atlas; call `into_inner` on the returned `FileLock`. | `remove` returns the `FileLock`; its metadata has the entry's rows; the key is gone and the same key can be opened again on another path. |
| T-008 | `FA-FR-007`, `FA-FR-010` | Keep a handle from `get`, call `remove`, drop the handle, call `remove` again. | The first call returns `FileAtlasError::InUse` and the entry stays; the second succeeds. |
| T-009 | `FA-RS-004` | Use a `FileAtlas<String>`; call `get`, `contains`, `with_read`, and `remove` with `&str` keys. | All calls compile and behave as with `&String`. |
| T-010 | `FA-RS-005` | Check the `Display` text of every `FileAtlasError` variant, and that `downcast_ref` recovers each from an `anyhow::Error`. | Messages match the documented text; `source()` is `None`; downcasting works. |
| T-011 | `FA-FR-007` | `remove` a missing key; `remove` an entry whose `FileLock` is poisoned (poisoned through a handle in a panicking thread). | Missing key: `FileAtlasError::KeyNotFound`. Poisoned entry: `remove` succeeds and the returned `FileLock`'s `recover` returns `Some(report)`. |
| T-012 | `FA-FR-008` | `with_write` appends rows; `with_read` reads them back; a closure returning `Err`; both on a missing key. | Rows are visible to `with_read`; the closure error is returned unchanged; missing keys return `FileAtlasError::KeyNotFound`. |
| T-013 | `FA-FR-009` | `with_struct_write::<Record>` writes and inserts records; `with_struct_read::<Record>` iterates them; both with a record type of another size. | Records round-trip in order; the wrong-size call returns the `StructFile` row-width error and does not call the closure. |
| T-014 | `FA-FR-008`, `FA-RS-002` | Inside a `with_write` closure on key A, call `open_or_create` for key B, `get` for A and B, `contains`, `len`, `keys`, and `with_read` on B. | Every call returns without deadlock and with the expected result. |
| T-015 | `FA-FR-007`, `FA-FR-008` | While a `with_read` closure is running on one thread (held at a `Barrier`), another thread calls `remove` on the same key. | `remove` returns `FileAtlasError::InUse`; after the closure returns, `remove` succeeds. |
| T-016 | `FA-FR-011`, `FA-RS-001` | Share `Arc<FileAtlas<u32>>`: one thread holds a write guard on key 1 (through a handle) at a `Barrier` while other threads read and write key 2 and open key 3. Several reader threads hold read guards on key 2 at the same time. | All other operations complete while key 1 is write-locked; concurrent readers on key 2 all pass the barrier. |
| T-017 | `FA-FR-003` | Several threads race to `open_or_create` the same new key on different paths, starting together at a `Barrier`. | Exactly one succeeds; all others return `FileAtlasError::KeyAlreadyPresent`; `len() == 1`; the losing threads' files are not locked afterwards. |
| T-018 | `FA-RS-003` | Use a key type whose `Hash` panics for one chosen value; call `remove` with it inside `catch_unwind` (`remove` hashes under the map write lock, so the panic poisons it); then use the atlas normally. | The panic is caught; afterwards `open_or_create`, `get`, `len`, and `remove` work as before. |
| T-019 | `FA-DS-001`, `FA-RS-001` | Compile-time assertion that `FileAtlas<String>: Send + Sync`. Write rows through the atlas, `remove` and drop the entry, then reopen the path with plain `FileMetadata::open_or_create`. | The assertion compiles; the reopened file has the same rows and no other file is in the directory. |
| T-020 | `FA-FR-012`, `FA-RS-008` | Open three entries; call `iter`, check `len()` before and while consuming it; open a fourth entry and remove one before consuming the rest. | The snapshot yields exactly the three original keys, each handle `Arc::ptr_eq` to `get` of its key; `len()` counts down exactly; later opens and removals do not appear; after the end, `next()` keeps returning `None`. |
| T-021 | `FA-FR-013`, `FA-FR-015` | Loop with `for (key, handle) in &atlas`; inside the loop, read through the handle, call `open_or_create` for a new key, `with_write` on another key, and `remove` on the current key. | The loop compiles and finishes without deadlock; `remove` inside the loop returns `FileAtlasError::InUse`; after the loop and its handles are dropped, `remove` succeeds. |
| T-022 | `FA-FR-014`, `FA-RS-008` | Consume an atlas with two entries by value (`atlas.into_iter()`, check `len()`, then loop), keep one handle, drop the other; check the OS locks with a plain `File::try_lock`. | Both entries are yielded; the dropped entry's file can be locked at once; the kept entry's file stays locked until its handle is dropped. |
| T-023 | `FA-RS-008` | Compile-time assertions that `FileAtlasIter<String>` and `FileAtlasIntoIter<String>` are `Send + Sync`, `ExactSizeIterator`, and `FusedIterator`. | The assertions compile. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `rust_serial_file/tests/file_atlas.rs` | `new_and_default_atlas_are_empty` |
| T-002 | `rust_serial_file/tests/file_atlas.rs` | `open_or_create_stores_and_shares_handles` |
| T-003 | `rust_serial_file/tests/file_atlas.rs` | `opening_an_existing_key_is_rejected` |
| T-004 | `rust_serial_file/tests/file_atlas.rs` | `opening_a_locked_path_is_rejected` |
| T-005 | `rust_serial_file/tests/file_atlas.rs` | `os_lock_is_held_until_removed_lock_is_dropped` |
| T-006 | `rust_serial_file/tests/file_atlas.rs` | `open_struct_uses_the_record_size` |
| T-007 | `rust_serial_file/tests/file_atlas.rs` | `remove_returns_the_owned_file_lock` |
| T-008 | `rust_serial_file/tests/file_atlas.rs` | `remove_fails_while_a_handle_is_held` |
| T-009 | `rust_serial_file/tests/file_atlas.rs` | `lookups_accept_borrowed_keys` |
| T-010 | `rust_serial_file/tests/file_atlas.rs` | `atlas_errors_display_and_downcast` |
| T-011 | `rust_serial_file/tests/file_atlas.rs` | `remove_reports_missing_keys_and_returns_poisoned_locks` |
| T-012 | `rust_serial_file/tests/file_atlas.rs` | `metadata_closures_read_write_and_propagate_errors` |
| T-013 | `rust_serial_file/tests/file_atlas.rs` | `struct_closures_read_write_and_check_row_width` |
| T-014 | `rust_serial_file/tests/file_atlas.rs` | `closures_can_use_the_atlas_without_deadlock` |
| T-015 | `rust_serial_file/tests/file_atlas.rs` | `running_closure_blocks_remove` |
| T-016 | `rust_serial_file/tests/file_atlas.rs` | `entries_are_locked_independently_across_threads` |
| T-017 | `rust_serial_file/tests/file_atlas.rs` | `racing_opens_of_one_key_have_one_winner` |
| T-018 | `rust_serial_file/tests/file_atlas.rs` | `atlas_survives_a_panicking_key_hash` |
| T-019 | `rust_serial_file/tests/file_atlas.rs` | `atlas_is_send_sync_and_adds_no_files` |
| T-020 | `rust_serial_file/tests/file_atlas.rs` | `iter_returns_a_snapshot_of_entries` |
| T-021 | `rust_serial_file/tests/file_atlas.rs` | `borrowed_iteration_allows_atlas_calls_and_blocks_remove` |
| T-022 | `rust_serial_file/tests/file_atlas.rs` | `owned_iteration_releases_files_as_handles_drop` |
| T-023 | `rust_serial_file/tests/file_atlas.rs` | `atlas_iterators_have_the_expected_traits` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-002 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-003 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-004 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-005 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-006 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-007 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-008 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-009 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-010 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-011 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-012 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-013 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-014 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-015 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-016 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-017 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-018 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-019 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-020 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-021 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-022 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |
| T-023 | 2026-11-11 | `edf83b6+dirty` | ✅ Success |

#### Notes

- Environment: Windows 10, `cargo 1.95.0`, run from `rust_serial_file/`.
  Command: `cargo test --test file_atlas`. The P-004 reliability criterion
  was met: 10 runs in a row, all 23 tests passed every time (about 0.01 s
  per run). The full crate suite (`cargo test`, 207 tests including doc
  tests) passes, as do `cargo fmt --check` and `cargo clippy --all-targets`
  with no warnings.
- T-004, T-005, T-017, and T-022 rely on Windows `LockFileEx` behavior: a
  second handle in the same process cannot take the lock. The same holds for
  `flock` on Unix, but this was not run on Unix.
- T-011 panics on purpose in a spawned thread, and T-018 inside
  `catch_unwind`; their panic messages are expected.

#### Warning notes

- None.

### Acceptance criteria

1. `FileAtlas<K>` opens files under generic `Hash + Eq` keys, holds an
   exclusive OS lock on each, and is shareable across threads.
2. Handles, metadata closures, and typed `StructFile` closures give read and
   write access, with the map lock never held during file access.
3. `remove` refuses while handles exist and returns the owned `FileLock`
   otherwise; the OS lock is released when it is dropped.
4. Every failure in [Errors and failure behavior](#errors-and-failure-behavior)
   is reported as an error and tested; no call panics on a lock state.
5. `FileLock`, `FileMetadata`, and `StructFile` are unchanged.
6. `cargo fmt --check`, `cargo clippy --all-targets`, `cargo build`, and
   `cargo test` pass.
7. Coverage is measured for the touched files and uncovered code is
   justified.
8. The atlas can be iterated by reference (snapshot, no map lock during the
   loop) and by value, with `Iterator`, `ExactSizeIterator`, and
   `FusedIterator`; `Index` and `SliceIndex` are documented as not
   implementable.

## Code coverage

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** measured in P-006 over the full test suite.
  `src/file/file_atlas.rs` has 97.42% line coverage; the only source lines
  never executed are the lock-error branch for errors other than
  `WouldBlock` (see the notes). The test file `tests/file_atlas.rs` is test
  code and is not reported.

### Current coverage results

- **Tool and command:** `cargo-llvm-cov 0.9.1` with `llvm-tools-preview`, on
  the stable toolchain, run from `rust_serial_file/`: `cargo llvm-cov clean`,
  then `cargo llvm-cov --all-targets`, then
  `cargo llvm-cov report --show-missing-lines`. Branch coverage is `N/A`:
  `--branch` needs a nightly compiler.
- **Date of run:** 2026-11-11
- **Git hash:** `edf83b6+dirty`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `rust_serial_file/src/file/file_atlas.rs` | 89.66% (26/29) | 97.42% (189/194) | 88.94% (201/226) | N/A |
| `rust_serial_file/src/file/mod.rs` | N/A | N/A | N/A | N/A |

### Coverage notes

- **`rust_serial_file/src/file/file_atlas.rs`:**
  - `--show-missing-lines` reports lines 166-167 as never executed: the
    `TryLockError::Error` branch of `open_or_create`, reached only when
    `try_lock` fails for a reason other than the file being locked (for
    example a platform without file locking). It cannot be triggered from a
    test on Windows or Unix without fault injection.
  - The 3 uncovered functions are the `with_context` closures of
    `open_or_create` at lines 162, 167, and 171. The closures at 167 (lock
    error) and 171 (`FileMetadata::new` failing after a successful open and
    lock, which needs `File::metadata` to fail) cannot be triggered without
    fault injection. The closure at 162 (open failure) is run by T-002, which
    checks its message, and line 162 is reported as covered; llvm-cov still
    counts that closure as unexecuted, which looks like a closure-attribution
    quirk of the tool.
  - The other missed lines and regions are per-instantiation counts of the
    generic methods: the library and each test key type (`String`, `u32`,
    `PanicKey`) compile them separately, and not every instantiation runs
    every path.
  - No further plan; covering the two error closures needs a fault-injection
    seam in `FileMetadata` or the file opening.
- **`rust_serial_file/src/file/mod.rs`:** only module declarations (one
  `pub mod file_atlas;` line added), with no executable code, so the tool
  reports no metrics.

## Out of scope

- **Owned guards and returned views:** guards that own an `Arc<FileLock>`
  (for example `read_arc` / `write_arc`), so the atlas could return a ready
  `StructFile` from one call. `std::sync::RwLock` has no owned guards, so this
  needs either `unsafe` code in `FileLock` (forbidden by `FL-RS-005`) or a
  different lock crate (such as `lock_api` with `arc_lock`, which has no
  poisoning, so `FileLock`'s poison tracking would have to be rebuilt).
  Decision deferred. Owner: the user, as a future `FileLock` revision.
- **Shared OS locks and read-only entries:** opening a file with a shared OS
  lock so other processes can read it at the same time, and an entry kind
  that refuses writes. Owner: the user, if a multi-process reader use
  appears.
- **Blocking or timed OS lock acquisition** (`File::lock`, retries).
- **Inserting an already opened `FileMetadata` or `FileLock`:** the atlas
  cannot take the OS lock on a `File` it did not open, because
  `FileMetadata` does not expose its `File`. Owner: the user; it would need a
  `FileMetadata` change.
- **Forced removal** of an entry that still has handles, and replacing an
  entry under an existing key.
- **Declaring `rust-version = "1.89"` in `Cargo.toml`.** This is a crate-wide
  decision that affects downstream projects. Owner: the user.
- Path canonicalization or detecting the same file under two paths other
  than through the OS lock.
- **`std::ops::Index` / `IndexMut` (`atlas[&key]`):** not possible safely.
  `Index::index` must return `&Output` borrowed from `&self` for as long as
  the caller holds it, but entries live behind the map's `RwLock`; a
  reference into the map would outlive the map guard, and another thread
  could remove or move the entry meanwhile. Returning `&FileLock` would need
  `unsafe` code or holding the map lock for the reference's whole life,
  which `FA-RS-002` forbids. `get` returns an owned handle instead
  (`FA-FR-016`).
- **`std::slice::SliceIndex`:** cannot be implemented outside the standard
  library (the trait is sealed), and it applies only to slices and `str`.
  Range access also has no meaning for an unordered `HashMap`.
- `FromIterator` / `Extend` for the atlas: opening an entry needs a path, an
  element size, and an offset, and can fail, which these traits cannot
  express.
- Atlas-level transactions across several files, persistence of the atlas
  itself, and async (`tokio`) access.
