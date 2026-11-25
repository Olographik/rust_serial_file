# File Sync Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.2
- **Short description:** Add explicit durability points (`sync_data` / `sync_all`) to `FileMetadata`, `BufferedWriter`, `StructFile`, `FileLock` and `FileAtlas`, so callers can persist written rows without closing the file.

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

Today the library has no durability point:

- `FileMetadata` writes with `seek` + `write_all` on a `std::fs::File`. The
  bytes reach the operating-system page cache but are never forced to the
  storage device. Dropping a `File` closes it **without** `fsync`, so even
  "close" is not a durability point; data written just before a crash or
  power loss can be lost.
- `BufferedWriter` keeps rows in memory until its cache is full, `flush` is
  called, or it is dropped (best-effort, errors lost). `flush` only hands rows
  to `FileMetadata` (`RSF-BW-DS-008`).
- `FileMetadata::reserve_rows` extends the file with zero rows. The reserved
  rows are hidden in memory, but nothing trims them: after the process ends,
  reopening the file counts them as real rows.

This document adds explicit `sync_data` and `sync_all` operations through
every layer, and makes a sync leave the file in a state that reopens with the
same logical row count.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-25 | Olographik | Implemented (P-001 to P-005). Modified `SYN-RSF-FR-003` (the reservation is released only while the file still ends at this handle's reservation) and `SYN-RSF-FR-007` (atlas syncs use `sync_data`; failures are reported as `FileAtlasSyncError<K>`, which needs `K: Clone + Debug + Send + Sync + 'static`). Added test T-011. Public API, decisions, checklist, test results, notes, coverage and reflection updated; related documents point to this one. |

## Goals and scope

### Goals

- A caller can make every successful write so far durable, without closing
  the file, from any writable layer (`FileMetadata`, `BufferedWriter`,
  `StructFile`, `FileLock`, `FileAtlas`).
- After a successful sync, reopening the file (even after a crash) yields the
  same row count and bytes as the in-memory view at sync time.
- Sync errors are always returned to the caller.

### Scope

- `src/file/file_metadata.rs`, `src/file/buffered_writer.rs`,
  `src/file/struct_file.rs`, `src/file/file_lock.rs`,
  `src/file/file_atlas.rs`.
- Unchanged: write semantics, row layout, the in-memory behavior of
  `reserve_rows`, the best-effort `Drop` of `BufferedWriter`.

## Assumptions and dependencies

- `std::fs::File::sync_data` and `sync_all` are the durability primitives
  (`fdatasync`/`fsync` on Unix, `FlushFileBuffers` on Windows). The guarantee
  is what the OS and the device give for these calls.
- No new crate dependency.
- Edition 2024, as the rest of `rust_serial_file`.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `SYN-RSF-FR-001` | `FileMetadata` shall provide `sync_data(&mut self) -> Result<()>`, which makes the file content and length durable (`File::sync_data`). | 0.1 |
| `SYN-RSF-FR-002` | `FileMetadata` shall provide `sync_all(&mut self) -> Result<()>`, which also makes file metadata (timestamps, permissions) durable (`File::sync_all`). | 0.1 |
| `SYN-RSF-FR-003` | Before syncing, `FileMetadata` shall release reserved rows beyond the logical row count (truncate the file to `data_offset + element_count * element_size`), so the synced file reopens with the same row count. It shall do so only when the file still ends at this handle's reservation; if another handle changed the file length, nothing is truncated. | 0.2 |
| `SYN-RSF-FR-004` | `BufferedWriter` shall provide `sync_data` and `sync_all`, which first `flush` pending rows, then sync the borrowed `FileMetadata`. If the flush fails, the sync is not attempted and pending rows are kept. | 0.1 |
| `SYN-RSF-FR-005` | `StructFile<T, M>` shall provide `sync_data` and `sync_all` for `M: BorrowMut<FileMetadata>`; read-only views (`StructReadOnlyFile`) shall not offer them. | 0.1 |
| `SYN-RSF-FR-006` | `FileLock` shall provide `sync_data` and `sync_all`, which take the write lock for the duration of the sync, and report a poisoned lock as an error, like `write`. | 0.1 |
| `SYN-RSF-FR-007` | `FileAtlas` shall provide `sync(key)` for one file and `sync_all_files()` for every file it owns, both with `FileLock::sync_data`. `sync_all_files` shall try every file even after a failure and return a `FileAtlasSyncError<K>` listing every key that failed with its error. | 0.2 |
| `SYN-RSF-FR-008` | Calling sync with nothing written since the last sync shall succeed and leave the file unchanged. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `SYN-RSF-DS-001` | Sync shall not change the row layout, the metadata prefix, or the bytes of any logical row. | 0.1 |
| `SYN-RSF-DS-002` | After a successful sync, `FileMetadata::open_or_create` on the same path with the same `element_size` and `data_offset` shall report the same `element_count` as the synced handle. | 0.1 |
| `SYN-RSF-DS-003` | Writes done after the last successful sync have no durability guarantee; the documentation shall say so. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `SYN-RSF-RS-001` | Sync methods take `&mut self` on `FileMetadata`, `BufferedWriter` and writable `StructFile` (because of the reservation trim, `SYN-RSF-FR-003`), and `&self` on `FileLock` and `FileAtlas` (they lock internally). | 0.1 |
| `SYN-RSF-RS-002` | All sync methods return `anyhow::Result<()>`; no sync error is ignored or converted into success. | 0.1 |
| `SYN-RSF-RS-003` | `FileAtlas::sync_all_files` shall not hold the atlas map lock while syncing files, so other files stay usable. | 0.1 |
| `SYN-RSF-RS-004` | No `unsafe` code. | 0.1 |

## Public API

```rust
impl FileMetadata {
    pub fn sync_data(&mut self) -> Result<()>;
    pub fn sync_all(&mut self) -> Result<()>;
}

impl BufferedWriter<'_> {
    pub fn sync_data(&mut self) -> Result<()>; // flush + FileMetadata::sync_data
    pub fn sync_all(&mut self) -> Result<()>;
}

impl<T: SerializedStruct, M: BorrowMut<FileMetadata>> StructFile<T, M> {
    pub fn sync_data(&mut self) -> Result<()>;
    pub fn sync_all(&mut self) -> Result<()>;
}

impl FileLock {
    pub fn sync_data(&self) -> Result<()>; // write lock + sync
    pub fn sync_all(&self) -> Result<()>;
}

impl<K: Hash + Eq> FileAtlas<K> {
    pub fn sync<Q>(&self, key: &Q) -> Result<()>;           // FileLock::sync_data
    pub fn sync_all_files(&self) -> Result<()>              // every file, sync_data
    where
        K: Clone + fmt::Debug + Send + Sync + 'static;
}

/// Returned inside anyhow::Error by sync_all_files; downcast to read it.
pub struct FileAtlasSyncError<K> {
    pub failures: Vec<(K, anyhow::Error)>,
}
```

Typical use: write a batch, then `file.sync_data()?` as a commit point.

## Errors and failure behavior

- I/O errors from `set_len`, `sync_data` or `sync_all` are returned with
  context naming the operation.
- After a failed sync, the durability of earlier writes is unknown; the
  documentation shall say the caller must treat the file as possibly not
  persisted (on Linux, a failed `fsync` can drop dirty pages, so retrying may
  report success without the data).
- `FileAtlas::sync` on an unknown key returns an error.
- `FileLock` sync on a poisoned lock returns the poisoned-lock error.

## Safety and security

- None: no `unsafe` code is allowed.
- No new handling of untrusted input.

## Compatibility

- Additive API only; no existing signature changes.
- Behavior change: a sync trims rows reserved with `reserve_rows` from the
  file on disk. In memory, the next append re-extends the file as needed.
  Code that relied on the on-disk reservation surviving a sync would see a
  smaller file; no such use is known.
- The `BufferedWriter` requirement `RSF-BW-DS-008` (flush is not a durability
  point) stays true; sync is the new durability point.

## Performance and resource use

- A sync is a blocking system call whose cost depends on the device (from
  microseconds on NVMe with write cache to tens of milliseconds on HDD).
  Calling it after every row is the caller's choice; the library never syncs
  implicitly.
- `sync_data` is preferred over `sync_all` for row data; it avoids flushing
  timestamps.
- Non-goal: asynchronous or background sync.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **FileMetadata sync:** add `sync_data`/`sync_all` with the reservation trim. | None | T-001 to T-004 pass. |
| P-002 | **BufferedWriter and StructFile sync:** flush-then-sync on the writer; delegate on `StructFile`. | P-001 | T-005 to T-007 pass. |
| P-003 | **FileLock and FileAtlas sync:** lock-then-sync; atlas iterates a snapshot of its files and aggregates errors. | P-001 | T-008 to T-010 pass. |
| P-004 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-003 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-005 | **Documentation:** update this requirements document, the related documents (`file_metdata.md`, `buffered_writer.md`, `struct_file.md`, `file_lock.md`, `file_atlas.md`) and the code comments of the touched files. | P-004 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [x] P-001 — `FileMetadata sync` — Done
- [x] P-002 — `BufferedWriter and StructFile sync` — Done
- [x] P-003 — `FileLock and FileAtlas sync` — Done
- [x] P-004 — `Coverage of touched files` — Done
- [x] P-005 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- Every layer delegates to `FileMetadata`, so it comes first; P-002 and P-003
  are independent of each other.
- Risk: a real crash cannot be simulated in tests; tests check what can be
  observed (return values, trimmed length, reopen count). Durability itself
  relies on the OS contract.
- Confirmed with the approval of the document (2026-11-25): trimming the
  reservation on sync (`SYN-RSF-FR-003`); no automatic sync policy in this
  version (see Out of scope).

**Implementation reflection (update as work progresses):**

- Implemented on 2026-11-25; no phase added or reordered.
- `SYN-RSF-FR-003` was narrowed: `element_count` falls back to the file
  length when the file was changed through another handle, and in that case
  this handle no longer owns a reservation. Truncating then could cut rows
  written by the other handle, so the sync leaves the length alone (T-011).
- T-003 also shows the bug this fixes: before the sync, a reopen counts the
  100 reserved zero rows as real rows.
- `FileAtlasError` deliberately carries no key, but `SYN-RSF-FR-007` asks for
  the failing keys, so `sync_all_files` returns a separate error type,
  `FileAtlasSyncError<K>`, generic over the key. Its bounds are only on
  `sync_all_files`, so atlases with other key types are unaffected.
- `FileLock::sync_data` goes through a normal write guard, so `generation`
  increases; this is correct, since the sync can change the file length.
- The first version of T-005 assumed two non-contiguous rows stay cached;
  they do not (a write outside the window flushes), so the test was changed
  to contiguous rows. The code was not changed.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `SYN-RSF-FR-001`, `SYN-RSF-DS-002` | Append rows, `sync_data`, open a second handle on the path. | Second handle sees the same count and bytes. |
| T-002 | `SYN-RSF-FR-002` | Same as T-001 with `sync_all`. | Same result. |
| T-003 | `SYN-RSF-FR-003`, `SYN-RSF-DS-001` | `reserve_rows(100)` on a 3-row file, sync, check `file_len` and reopen. | File length is `data_offset + 3 * element_size`; reopen sees 3 rows with unchanged bytes; later appends still work. |
| T-004 | `SYN-RSF-FR-008` | Sync twice with no write in between. | Both succeed; file bytes unchanged. |
| T-005 | `SYN-RSF-FR-004` | Buffer rows in `BufferedWriter`, `sync_data`, read through a second handle while the writer is alive. | Rows are visible; writer cache is empty. |
| T-006 | `SYN-RSF-FR-004` | Force a flush failure (row beyond logical end) then sync. | Sync returns the flush error; no sync attempted; pending rows kept. |
| T-007 | `SYN-RSF-FR-005` | `StructFile` write then `sync_data`; check `StructReadOnlyFile` has no sync (compile-fail doc test). | Sync works; doc test fails to compile as expected. |
| T-008 | `SYN-RSF-FR-006` | `FileLock::sync_data` after writes; on a poisoned lock. | Success; poisoned lock returns an error. |
| T-009 | `SYN-RSF-FR-007` | `FileAtlas::sync` on a known and an unknown key. | Known key succeeds; unknown key errors. |
| T-010 | `SYN-RSF-FR-007`, `SYN-RSF-RS-003` | `sync_all_files` with one poisoned file among three. | The two healthy files are synced; the error names the poisoned key. |
| T-011 | `SYN-RSF-FR-003` | Reserve rows, shorten the file through a second handle, then sync the first. | The sync succeeds and truncates nothing; the length set by the other handle is kept. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `rust_serial_file/tests/file_metadata.rs` | `sync_data_persists_rows` |
| T-002 | `rust_serial_file/tests/file_metadata.rs` | `sync_all_persists_rows` |
| T-003 | `rust_serial_file/tests/file_metadata.rs` | `sync_trims_reserved_rows` |
| T-004 | `rust_serial_file/tests/file_metadata.rs` | `sync_without_writes_is_noop` |
| T-005 | `rust_serial_file/tests/buffered_writer.rs` | `sync_flushes_pending_rows` |
| T-006 | `rust_serial_file/tests/buffered_writer.rs` | `sync_reports_flush_error` |
| T-007 | `rust_serial_file/tests/struct_file.rs` + doc test in `src/file/struct_file.rs` | `struct_file_sync_data` |
| T-008 | `rust_serial_file/tests/file_lock.rs` | `lock_sync_and_poison` |
| T-009 | `rust_serial_file/tests/file_atlas.rs` | `atlas_sync_one` |
| T-010 | `rust_serial_file/tests/file_atlas.rs` | `atlas_sync_all_reports_failures` |
| T-011 | `rust_serial_file/tests/file_metadata.rs` | `sync_keeps_file_changed_by_another_handle` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-002 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-003 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-004 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-005 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-006 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-007 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-008 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-009 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-010 | 2026-11-25 | `4b898c1` | ✅ Success |
| T-011 | 2026-11-25 | `4b898c1` | ✅ Success |

#### Notes

- Command: `cargo test` in `rust_serial_file/` (214 tests, including doc
  tests, pass; 12 are new); `rust_python_file` `cargo test` (7) still passes.
  `cargo fmt --check` and `cargo clippy --all-targets` are clean.
- T-007: the compile-fail doc test is on `StructFile::sync_data` in
  `src/file/struct_file.rs`.
- No test can show durability itself (that would need a power cut); the
  tests check what is observable: return values, file length and bytes seen
  through another handle, and the row count after a reopen.

#### Warning notes

- None.

### Acceptance criteria

1. Every writable layer exposes `sync_data`/`sync_all` and returns errors.
2. A synced file reopens with the same rows.
3. Existing tests pass unchanged.
4. `cargo fmt`, `cargo clippy`, build and tests pass.

## Code coverage

- **Coverage summary:** every new function is covered; the remaining gaps in the new code are error paths that cannot be triggered portably. The other gaps are in code this work did not change (see notes).

### Current coverage results

- **Tool and command:** `cargo llvm-cov --all-targets --summary-only` (cargo-llvm-cov 0.9.2) in `rust_serial_file/`
- **Date of run:** 2026-11-25
- **Git hash:** `4b898c1`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `rust_serial_file/src/file/buffered_writer.rs` | 100.00% (14/14) | 100.00% (145/145) | 92.37% (218/236) | N/A |
| `rust_serial_file/src/file/file_atlas.rs` | 91.18% (31/34) | 97.30% (216/222) | 89.77% (237/264) | N/A |
| `rust_serial_file/src/file/file_lock.rs` | 100.00% (23/23) | 100.00% (115/115) | 98.17% (161/164) | N/A |
| `rust_serial_file/src/file/file_metadata.rs` | 93.02% (40/43) | 95.10% (388/408) | 84.93% (665/783) | N/A |
| `rust_serial_file/src/file/struct_file.rs` | 100.00% (34/34) | 95.53% (235/246) | 84.55% (372/440) | N/A |

### Coverage notes

Branch coverage is `N/A` for every file (needs a nightly toolchain). In the
new sync code, the only missed regions are in `file_metadata.rs` and
`file_atlas.rs`: `?` error paths of `set_len` failing while releasing a
reservation, the file length being unreadable, a `row_end` overflow that
cannot happen once the reservation itself succeeded, and the formatter
failing in `FileAtlasSyncError`'s `Display`. (`File::sync_data`/`sync_all`
failing returns directly, without a separate region.) None can be triggered portably from a
test (a reservation needs a writable handle, and `fsync` does not fail on a
healthy file system). The other gaps below predate this work and are listed
so the table can be read; no plan to cover them as part of it.

- **`rust_serial_file/src/file/buffered_writer.rs`:** all lines covered, and
  the new `sync_data`/`sync_all` have no missed region (their flush error is
  T-006). The 18 missed regions are older `?` error paths: constructor
  conversions and allocation (lines 23-53), block and window overflow checks
  (lines 95-146, 190-193) and the flush calls of the write paths (108, 132,
  200).
- **`rust_serial_file/src/file/file_atlas.rs`:** line 109, the `?` after
  `write!` in `FileAtlasSyncError`'s `Display` (only runs if the formatter
  fails); lines 196-197, the OS lock returning an error other than
  `WouldBlock` (older code). The 3 missed functions are closures of that
  error path and of generic instantiations not used by the tests.
- **`rust_serial_file/src/file/file_lock.rs`:** all lines covered, and the
  new `sync_data`/`sync_all` have no missed region (the poisoned-lock error is
  T-008). The 3 missed regions are older: the `?` in `FileSnapshot::capture`
  (lines 60-61) and in `FileWriteGuard::new` (line 324), which only fail if
  the file length cannot be read.
- **`rust_serial_file/src/file/file_metadata.rs`:** new code: the error
  regions described above (lines 302, 315, 328, 331, 334). Older code: line
  263 (`insert_row` offset overflow), lines 491-492 and 499-500 (`FileRows`
  allocation failure and end of rows), lines 535-550 (`read_exact_at`
  short-read, overflow and interrupted-read paths).
- **`rust_serial_file/src/file/struct_file.rs`:** the new `sync_data` and
  `sync_all` have no missed region. Line 417 (a row index not fitting in
  `u64`, impossible on 64-bit targets) and the other missed regions are older
  `?` error paths (conversions, allocations, overflow checks and I/O errors
  of the existing read and write methods).

## Out of scope

- Automatic sync policy (every N writes, every T seconds, on `FileWriteGuard`
  drop). Can be added later as a `SyncPolicy` option; decision: repository
  owner.
- Syncing the parent directory after creating a file (needed on Linux for the
  new file's directory entry to survive a crash). Decision: repository owner;
  it could be added to `open_or_create`.
- Trimming the reservation on `Drop` of `FileMetadata` (today reserved rows
  leak into the file if the process exits without a sync).
- Crash-consistency of multi-step operations (`insert_row`,
  `update_raw_metadata`, `truncate_rows_front` move data in place; a crash in
  the middle can leave a mixed state). Needs a journal or copy-on-write
  design.
- Dynamic (variable-length) storage; separate design, still under discussion.
