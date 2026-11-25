# Dynamic Storage Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Store variable-length byte values and strings in a companion blob file, referenced from fixed-width struct-file rows by `BlobRef { offset, len }`, with free-space reuse and an explicit compaction (defragmentation) function.

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

`StructFile` rows are fixed-width (`SerializedStruct::SIZE`), which keeps
indexing and binary search simple, but cannot hold strings or other
variable-length values. This document adds a **blob file**: a companion file
holding variable-length byte values. A row stores a `BlobRef { offset, len }`
(12 bytes, defined in the `serialized_struct` crate, `SSC-SST-FR-008`) that
points to its value in the blob file.

When a value grows beyond the space it was given, it is written to a new
place (a free block if one fits, else appended), and the old place becomes
free. Free blocks are tracked so they can be reused, and a compaction
function rewrites the blob file without holes and updates every `BlobRef` in
the rows.

Design decisions taken with the repository owner:

- Dynamic values are updated rarely; most are written once.
- A blob file belongs to exactly one struct file. Access to both goes through
  one owner: the `FileAtlas`, or a single paired file object.
- Only byte values and UTF-8 strings are in scope. Dynamic structures (a
  variable-length encoding of user structs) are on hold.
- Keeping the struct file and the blob file consistent with each other
  (write order, sync points) is the responsibility of the user of the files.
  The library provides the primitives and documents a safe order; it adds no
  journal or transaction.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|

## Goals and scope

### Goals

- Store, read, replace and free variable-length byte values and strings.
- Reuse freed space; never move a value that was not asked to move, except
  during compaction.
- Defragment on demand, updating the rows' `BlobRef`s.
- Open a struct file and its blob file together through `FileAtlas` or a
  paired type, under one lock.

### Scope

- New `src/file/blob_file.rs` (`BlobFile`), new
  `src/file/struct_blob_file.rs` (paired type and compaction), changes to
  `src/file/file_lock.rs` and `src/file/file_atlas.rs` for the pairing.
- `BlobRef` itself is specified in the `serialized_struct` crate.
- Unchanged: `StructFile`, `FileMetadata` and existing files without a blob
  file.

## Assumptions and dependencies

- Depends on the `serialized_struct` crate extraction (`BlobRef`) and on the
  sync work (`file_sync.md`) for `sync_data` / `sync_all`.
- No new crate dependency.
- One process owns the files at a time (enforced by the `FileAtlas` OS lock,
  as for struct files).
- A single value is at most `u32::MAX` bytes (4 GiB − 1).
- Edition 2024.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `DYN-RSF-FR-001` | `BlobFile::open_or_create(path)` shall open or create a blob file, validate its header, and rebuild the free-space index by scanning block headers. | 0.1 |
| `DYN-RSF-FR-002` | `write(&mut self, bytes) -> Result<BlobRef>` shall store a value in the best-fitting free block whose capacity is at least `bytes.len()`, or append a new block when none fits, and return its `BlobRef`. An empty value returns `BlobRef::EMPTY` without storage. | 0.1 |
| `DYN-RSF-FR-003` | `read(&self, r: BlobRef) -> Result<Vec<u8>>` and `read_into(&self, r, buf: &mut [u8])` shall return the `r.len` bytes of the value. `read_string` shall return a `String` and fail on invalid UTF-8. | 0.1 |
| `DYN-RSF-FR-004` | `update(&mut self, r: BlobRef, bytes) -> Result<BlobRef>` shall overwrite in place when the block capacity is at least `bytes.len()`; otherwise write to a new block (`DYN-RSF-FR-002`) and then free the old block. The caller stores the returned `BlobRef` in its row. | 0.1 |
| `DYN-RSF-FR-005` | `free(&mut self, r: BlobRef)` shall mark the block free and merge it with adjacent free blocks. A free block at the end of the file shall be removed by truncating the file. Freeing `BlobRef::EMPTY` is a no-op. | 0.1 |
| `DYN-RSF-FR-006` | `stats(&self) -> BlobStats` shall report file length, live bytes, free bytes, number of free blocks and the largest free block, so the caller can decide when to compact. | 0.1 |
| `DYN-RSF-FR-007` | A trait `BlobFields` shall let a record expose its `BlobRef` fields: `fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef))`. It is implemented by hand for now (see Out of scope for a derive). | 0.1 |
| `DYN-RSF-FR-008` | `compact` shall rewrite the blob file of a struct file with rows `T: SerializedStruct + BlobFields`: copy every live value referenced by the rows, in row order, into a new blob file without holes, update and rewrite each row's `BlobRef`s, then replace the old blob file. Unreferenced blocks (leaks) are dropped. | 0.1 |
| `DYN-RSF-FR-009` | Compaction shall write the new blob file to `<blob path>.compact`, keep the previous files until the struct file and the blob file are both updated and synced, then rename. If a `.compact` file exists when a blob file is opened, the open shall fail with an error naming it, so the user decides how to recover. | 0.1 |
| `DYN-RSF-FR-010` | `StructBlobFile<T>` shall own a `StructFile<T>` and its `BlobFile` together, opened from two paths, with methods to access both mutably at once, `compact`, `sync_data` and `sync_all` (syncing the blob file first, then the struct file). | 0.1 |
| `DYN-RSF-FR-011` | `FileLock` shall be able to hold an optional `BlobFile` with its `FileMetadata`, under the same `RwLock`. Read guards give `&BlobFile`, write guards give `&mut BlobFile`, alongside the metadata. | 0.1 |
| `DYN-RSF-FR-012` | `FileAtlas` shall provide `open_struct_with_blob::<T>(key, path, blob_path, data_offset)`, which opens and OS-locks both files, and `with_struct_blob_read` / `with_struct_blob_write` closures that give a typed `StructFile` view and the `BlobFile`. Opening fails if either file is already locked. | 0.1 |
| `DYN-RSF-FR-013` | `BlobFile` shall provide `sync_data` and `sync_all` with the semantics of `file_sync.md`. | 0.1 |
| `DYN-RSF-FR-014` | `read`, `update` and `free` shall check that `r` designates the start of a live block with capacity `>= r.len`, and return an error otherwise, instead of reading or corrupting another value. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `DYN-RSF-DS-001` | File header (16 bytes): magic `RSFBLOB1` (8 bytes), format version `u16`, reserved `u16` and `u32` set to zero. All integers little-endian. | 0.1 |
| `DYN-RSF-DS-002` | Each block: an 8-byte header `state: u32` (`1` = live, `0` = free) and `capacity: u32` (payload bytes), followed by `capacity` payload bytes. Blocks are contiguous from offset 16 to the end of the file. | 0.1 |
| `DYN-RSF-DS-003` | `BlobRef.offset` is the file offset of the block **payload** (block header at `offset − 8`); `BlobRef.len` is the value length (`len <= capacity`). The value length is stored only in the row, as requested ("start and end"); the block header holds only state and capacity. | 0.1 |
| `DYN-RSF-DS-004` | New block capacity is `len` rounded up to a multiple of 8, giving small in-place growth room and keeping headers 8-byte aligned. A free block larger than needed by at least 16 bytes is split; otherwise the whole block is used. | 0.1 |
| `DYN-RSF-DS-005` | The free-space index is in memory only (by offset for merging, by capacity for best fit) and is rebuilt on open from block headers; it is never persisted, so it cannot disagree with the file. | 0.1 |
| `DYN-RSF-DS-006` | Opening fails with an error on a bad magic, an unknown version, or a block header running past the end of the file. Trailing bytes shorter than a block header are reported as an error, not ignored. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `DYN-RSF-RS-001` | Reads take `&self` (positioned reads, like `FileMetadata`); writes, updates, frees, compaction and sync take `&mut self`. `BlobFile` is `Send + Sync`. | 0.1 |
| `DYN-RSF-RS-002` | All fallible operations return `anyhow::Result`; buffers are allocated with `try_reserve` and allocation failure is an error. | 0.1 |
| `DYN-RSF-RS-003` | No `unsafe` code. | 0.1 |
| `DYN-RSF-RS-004` | Existing `FileLock` and `FileAtlas` APIs keep their signatures; the blob pairing is additive. | 0.1 |

## Public API

```rust
// serialized_struct crate (see serialized_struct_crate.md)
pub struct BlobRef { pub offset: u64, pub len: u32 }

// rust_serial_file::file::blob_file
pub struct BlobFile { /* file, free index */ }
pub struct BlobStats { pub file_len: u64, pub live_bytes: u64, pub free_bytes: u64,
                       pub free_blocks: usize, pub largest_free_block: u32 }

impl BlobFile {
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self>;
    pub fn write(&mut self, bytes: &[u8]) -> Result<BlobRef>;
    pub fn write_str(&mut self, s: &str) -> Result<BlobRef>;
    pub fn read(&self, r: BlobRef) -> Result<Vec<u8>>;
    pub fn read_into(&self, r: BlobRef, buf: &mut [u8]) -> Result<()>;
    pub fn read_string(&self, r: BlobRef) -> Result<String>;
    pub fn update(&mut self, r: BlobRef, bytes: &[u8]) -> Result<BlobRef>;
    pub fn free(&mut self, r: BlobRef) -> Result<()>;
    pub fn stats(&self) -> BlobStats;
    pub fn sync_data(&mut self) -> Result<()>;
    pub fn sync_all(&mut self) -> Result<()>;
}

pub trait BlobFields {
    fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef));
}

// rust_serial_file::file::struct_blob_file
pub struct StructBlobFile<T: SerializedStruct> { /* StructFile<T> + BlobFile */ }
impl<T: SerializedStruct> StructBlobFile<T> {
    pub fn open_or_create(path: impl AsRef<Path>, blob_path: impl AsRef<Path>, data_offset: u64) -> Result<Self>;
    pub fn parts_mut(&mut self) -> (&mut StructFile<T>, &mut BlobFile);
    pub fn sync_data(&mut self) -> Result<()>;
    pub fn sync_all(&mut self) -> Result<()>;
}
impl<T: SerializedStruct + BlobFields> StructBlobFile<T> {
    pub fn compact(&mut self) -> Result<BlobStats>;
}

// FileAtlas additions
impl<K: Hash + Eq> FileAtlas<K> {
    pub fn open_struct_with_blob<T: SerializedStruct>(&self, key: K, path: impl AsRef<Path>,
        blob_path: impl AsRef<Path>, data_offset: u64) -> Result<Arc<FileLock>>;
    pub fn with_struct_blob_write<T, Q, R>(&self, key: &Q,
        f: impl FnOnce(&mut StructWriteFile<'_, T>, &mut BlobFile) -> Result<R>) -> Result<R>;
    pub fn with_struct_blob_read<T, Q, R>(&self, key: &Q,
        f: impl FnOnce(&StructReadOnlyFile<'_, T>, &BlobFile) -> Result<R>) -> Result<R>;
    pub fn compact_blob<T: SerializedStruct + BlobFields, Q>(&self, key: &Q) -> Result<BlobStats>;
}
```

Recommended write order, documented for users (consistency is their
responsibility):

1. `blob.write` / `blob.update` → new `BlobRef`.
2. `blob.sync_data()`.
3. Write the row with the new `BlobRef`, then `struct_file.sync_data()`.
4. Only then `blob.free(old)` if the value moved (`update` does this itself;
   users wanting the strict order use `write` + `free`).

With this order a crash can leak space (recovered by `compact`) but never
leave a row pointing to garbage.

## Errors and failure behavior

- Invalid `BlobRef` (not a live block start, `len > capacity`, beyond end):
  error, nothing read or written (`DYN-RSF-FR-014`).
- Corrupt file on open: error (`DYN-RSF-DS-006`).
- Leftover `.compact` file on open: error naming it (`DYN-RSF-FR-009`).
- Value larger than `u32::MAX` bytes: error.
- Invalid UTF-8 in `read_string`: error; `read` still returns the bytes.
- Compaction failure before the renames: the original files are untouched
  and the `.compact` file is removed when possible; the error is returned.
- No error is converted into success; `Drop` performs no I/O.

## Safety and security

- None: no `unsafe` code is allowed.
- Blob files and `BlobRef`s may come from untrusted sources (for example rows
  received from the browser): every `BlobRef` is validated against the block
  headers before use, and lengths come from validated headers, so a crafted
  reference cannot read outside a value or cause a huge allocation beyond
  `u32::MAX`.

## Compatibility

- Additive: existing struct files, `StructFile`, `FileLock` and `FileAtlas`
  APIs are unchanged.
- The blob file format is versioned (`DYN-RSF-DS-001`) so dynamic structures
  can be added later without breaking files.
- Adding a `BlobRef` field to an existing record type changes its `SIZE`;
  migrating such files is the user's job.

## Performance and resource use

- `read`: one positioned read of the header plus one of the payload (could
  be one read of `8 + len` bytes).
- `write`/`update`: `O(log n)` in the free-index size plus the write.
- Open: one sequential scan of block headers, `O(number of blocks)` seeks. As
  updates are rare, the number of blocks is close to the number of values;
  acceptable for the target sizes. A persisted index is out of scope.
- `compact`: reads every live value once and rewrites the struct file; needs
  up to twice the blob file size on disk during the operation.
- Memory: the free index holds two entries per free block.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Blob file core:** header, block format, open with scan, append-only `write`, `read`, `read_string`, validation of `BlobRef`, `sync`. | `serialized_struct_crate.md` P-003 (`BlobRef`), `file_sync.md` P-001. | T-001 to T-005 pass. |
| P-002 | **Free space:** `free` with merging and tail truncation, best-fit reuse with splitting, `update` in place or move, `stats`. | P-001 | T-006 to T-009 pass. |
| P-003 | **Pairing and compaction:** `BlobFields`, `StructBlobFile`, `compact` with `.compact` file and renames, leftover detection. | P-002 | T-010 to T-012 pass. |
| P-004 | **Atlas integration:** optional `BlobFile` in `FileLock`, `FileAtlas::open_struct_with_blob`, closures, `compact_blob`. | P-003 | T-013, T-014 pass; existing atlas and lock tests pass. |
| P-005 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-004 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-006 | **Documentation:** update this requirements document, `file_lock.md`, `file_atlas.md`, and the code comments of the touched files (including the recommended write order). | P-005 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [ ] P-001 — `Blob file core` — Not started
- [ ] P-002 — `Free space reuse and update` — Not started
- [ ] P-003 — `StructBlobFile and compaction` — Not started
- [ ] P-004 — `FileAtlas and FileLock integration` — Not started
- [ ] P-005 — `Coverage of touched files` — Not started
- [ ] P-006 — `Documentation update` — Not started

**Planning reflection (complete before implementation):**

- P-001 alone is already usable (append-only storage, enough since updates
  are rare); P-002 adds reuse; P-003 needs both a row type and a blob file;
  P-004 only wires the existing pieces into the lock and atlas.
- This work starts after the `serialized_struct` crate extraction and the
  file sync work, which it depends on.
- Risks: compaction touches two files and cannot be atomic across them; the
  `.compact` file plus keeping the old files until both are synced limits
  the damage, and recovery is left to the user as agreed. Adding an optional
  `BlobFile` to `FileLock` must not change the existing guard API.
- Decisions to confirm: `BlobRef` = offset + length (12 bytes) instead of
  start + end pointers (16 bytes); capacity rounding to 8 bytes; error (not
  automatic recovery) on a leftover `.compact` file.

**Implementation reflection (update as work progresses):**

- None yet.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `DYN-RSF-FR-001`, `DYN-RSF-DS-001` | Create a blob file, check the header bytes, reopen. | Header matches; reopen succeeds with no blocks. |
| T-002 | `DYN-RSF-FR-002`, `DYN-RSF-FR-003`, `DYN-RSF-DS-002`, `DYN-RSF-DS-003`, `DYN-RSF-DS-004` | Write several values (including empty and non-multiple-of-8 lengths), read them back, reopen and read again. | Values equal; block headers and capacities as specified; empty value is `BlobRef::EMPTY`. |
| T-003 | `DYN-RSF-FR-003` | `write_str` / `read_string`, and `read_string` on invalid UTF-8 bytes. | Round trip equal; invalid UTF-8 returns an error. |
| T-004 | `DYN-RSF-FR-014` | Read/update/free with a `BlobRef` in the middle of a block, beyond the end, on a free block, with `len > capacity`. | Each returns an error; file unchanged. |
| T-005 | `DYN-RSF-DS-006` | Open files with bad magic, unknown version, truncated block, trailing partial header. | Each open returns an error. |
| T-006 | `DYN-RSF-FR-005` | Free middle blocks in various orders; free the last block. | Adjacent free blocks merged; file truncated when the tail is free. |
| T-007 | `DYN-RSF-FR-002`, `DYN-RSF-DS-004` | After frees, write values that fit exactly, fit with split, fit without split. | Best-fitting block reused; split only when the remainder is at least 16 bytes. |
| T-008 | `DYN-RSF-FR-004` | `update` with a smaller, same-capacity, and larger value. | In place when it fits (same `offset`); moved otherwise and old block free. |
| T-009 | `DYN-RSF-FR-006`, `DYN-RSF-DS-005` | `stats` after a scripted sequence, then reopen and compare `stats`. | Values match the expected numbers; same after reopen. |
| T-010 | `DYN-RSF-FR-007`, `DYN-RSF-FR-008`, `DYN-RSF-FR-010` | `StructBlobFile` with a record holding two `BlobRef`s; fragment it, leak a block, `compact`. | All rows read the same values; no free bytes; leaked block gone; file length equals the sum of live block sizes plus header. |
| T-011 | `DYN-RSF-FR-009` | Leave a `.compact` file next to a blob file and open. | Open fails with an error naming the file. |
| T-012 | `DYN-RSF-FR-010`, `DYN-RSF-FR-013` | `sync_data` on `StructBlobFile` and reopen both files. | Rows and values readable after reopen. |
| T-013 | `DYN-RSF-FR-011`, `DYN-RSF-FR-012`, `DYN-RSF-RS-004` | Open through `FileAtlas::open_struct_with_blob`, write and read with the closures from several threads; open the same paths twice. | Data consistent; second open fails as locked; existing atlas tests unchanged. |
| T-014 | `DYN-RSF-FR-012` | `FileAtlas::compact_blob` while readers wait on the lock. | Readers see the compacted state after the write lock is released. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `rust_serial_file/tests/blob_file.rs` | `creates_and_reopens_header` |
| T-002 | `rust_serial_file/tests/blob_file.rs` | `writes_and_reads_values` |
| T-003 | `rust_serial_file/tests/blob_file.rs` | `reads_strings` |
| T-004 | `rust_serial_file/tests/blob_file.rs` | `rejects_invalid_refs` |
| T-005 | `rust_serial_file/tests/blob_file.rs` | `rejects_corrupt_files` |
| T-006 | `rust_serial_file/tests/blob_file.rs` | `frees_and_merges_blocks` |
| T-007 | `rust_serial_file/tests/blob_file.rs` | `reuses_best_fit_block` |
| T-008 | `rust_serial_file/tests/blob_file.rs` | `updates_in_place_or_moves` |
| T-009 | `rust_serial_file/tests/blob_file.rs` | `stats_survive_reopen` |
| T-010 | `rust_serial_file/tests/struct_blob_file.rs` | `compacts_and_updates_rows` |
| T-011 | `rust_serial_file/tests/struct_blob_file.rs` | `refuses_leftover_compact_file` |
| T-012 | `rust_serial_file/tests/struct_blob_file.rs` | `sync_and_reopen` |
| T-013 | `rust_serial_file/tests/file_atlas.rs` | `atlas_struct_with_blob` |
| T-014 | `rust_serial_file/tests/file_atlas.rs` | `atlas_compact_blob` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | — | — | ⏳ Not run |
| T-002 | — | — | ⏳ Not run |
| T-003 | — | — | ⏳ Not run |
| T-004 | — | — | ⏳ Not run |
| T-005 | — | — | ⏳ Not run |
| T-006 | — | — | ⏳ Not run |
| T-007 | — | — | ⏳ Not run |
| T-008 | — | — | ⏳ Not run |
| T-009 | — | — | ⏳ Not run |
| T-010 | — | — | ⏳ Not run |
| T-011 | — | — | ⏳ Not run |
| T-012 | — | — | ⏳ Not run |
| T-013 | — | — | ⏳ Not run |
| T-014 | — | — | ⏳ Not run |

#### Notes

- None.

#### Warning notes

- None.

### Acceptance criteria

1. Strings and byte values can be stored, read, updated and freed through a
   `BlobRef` in a struct-file row.
2. Freed space is reused, and `compact` removes all holes and leaks while
   keeping every row's value.
3. A struct file and its blob file can be opened and used together through
   `FileAtlas` or `StructBlobFile`.
4. Existing behavior is unchanged; `cargo fmt`, `cargo clippy`, build and
   tests pass.

## Code coverage

- **Coverage summary:** Not run yet.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --all-targets --summary-only` (planned)
- **Date of run:** —
- **Git hash:** —

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `rust_serial_file/src/file/blob_file.rs` | — | — | — | — |
| `rust_serial_file/src/file/file_atlas.rs` | — | — | — | — |
| `rust_serial_file/src/file/file_lock.rs` | — | — | — | — |
| `rust_serial_file/src/file/struct_blob_file.rs` | — | — | — | — |

### Coverage notes

- Not run yet.

## Out of scope

- **Dynamic structures** (variable-length encoding of user structs, nested
  `BlobRef`s): on hold, needs a separate design reflection. Decision:
  Olographik.
- Sharing one blob file between several struct files.
- Crash consistency across the struct file and the blob file (journal,
  transactions): the user's responsibility, as agreed.
- Automatic compaction on open or on a fragmentation threshold: the user
  calls `compact` based on `stats`. Could be added as an open option later.
- A `#[derive(BlobFields)]` macro; to consider once the trait is stable.
- Persisting the free-space index to speed up opening very large blob files.
