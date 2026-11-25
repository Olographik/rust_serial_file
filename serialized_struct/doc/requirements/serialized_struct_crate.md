# Serialized Struct Crate Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.3
- **Short description:** Extract the `serialized_struct` module of `rust_serial_file` into its own dependency-light crate that builds for `wasm32-unknown-unknown`, so browser code and the host server can share record definitions without pulling `utils` or `rand`.

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

`EncodeRaw`, `DecodeRaw` and `SerializedStruct` (today in
`rust_serial_file/src/serialized_struct/mod.rs`) are the wire format used
between a WebAssembly browser client and a host server. To use them, the
browser build currently has to depend on the whole `rust_serial_file` crate,
which depends on `utils`, which depends on `rand` (and through it
`getrandom`). `getrandom` does not compile for `wasm32-unknown-unknown`
without extra configuration, and the file, lock and atlas modules are useless
in a browser anyway.

This document specifies a new workspace crate, `serialized_struct/`, that
owns these traits and their primitive implementations and depends on nothing
that is incompatible with WebAssembly. `rust_serial_file` depends on the new
crate and re-exports it at its existing path, so downstream code keeps
compiling. The derive macro in `serialized_struct_derive` keeps working with
both import styles.

Intended users: the WebAssembly client, the host server, and every existing
user of `rust_serial_file::serialized_struct`.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-25 | Olographik | Modified `SSC-SST-DS-002` (derived `usize`/`isize` fields are 64-bit, see `serialized_struct_derive/doc/requirements/pointer_sized_fields.md`). Added `SSC-SST-FR-008` (`BlobRef` handle type for dynamic storage) and tests T-008, T-009. Out of scope updated. |
| 0.3 | 2026-11-25 | Olographik | Implementation done (P-001 to P-005): checklist, test results, coverage and implementation reflection filled in. Crate name `serialized_struct` confirmed. P-003 completion criteria no longer include T-009, which needs the derive change of `pointer_sized_fields.md`. No requirement changed. |

## Goals and scope

### Goals

- A browser project can depend on `serialized_struct` (and optionally
  `serialized_struct_derive`) alone and build for `wasm32-unknown-unknown`.
- The byte format of every existing type and derived struct is unchanged, so
  files written before the split are read identically after it.
- Existing code using `rust_serial_file::serialized_struct::*` keeps
  compiling without edits.

### Scope

- New crate `serialized_struct/` holding the content of
  `rust_serial_file/src/serialized_struct/mod.rs` (traits, macros, primitive
  implementations) and its tests.
- `rust_serial_file`: replace the module by a re-export of the new crate.
- `serialized_struct_derive`: no change to generated code is required (see
  `SSC-SST-RS-003`); an optional `derive` feature on the new crate re-exports
  the macro.
- The existing requirements document
  `rust_serial_file/doc/requirements/serialized_struct/encode_decode_raw.md`
  moves to the new crate, keeping its IDs (`RSF-DS-…`, `RSF-RS-…`), which stay
  valid as historical IDs.
- Unchanged: trait names, method signatures, the little-endian encoding, and
  the `SIZE` constants.

## Assumptions and dependencies

- The browser build uses `wasm32-unknown-unknown` (via `wasm-bindgen` or
  `wasm-pack`); no WASI or Emscripten specific API is needed.
- Dependencies of the new crate: `anyhow` only (it builds for
  `wasm32-unknown-unknown`). Optional: `serialized_struct_derive` behind the
  `derive` feature.
- No dependency, direct or transitive, on `rand`, `getrandom`, `utils`, or any
  crate needing OS facilities.
- Rust edition 2024, like `rust_serial_file` and `utils`. No MSRV is declared
  beyond what edition 2024 requires.
- The `wasm32-unknown-unknown` target must be installed
  (`rustup target add wasm32-unknown-unknown`) to run the WebAssembly checks.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `SSC-SST-FR-001` | A new workspace crate named `serialized_struct` shall provide `EncodeRaw`, `DecodeRaw` and `SerializedStruct` at its crate root, with the same definitions and default methods as the current module. | 0.1 |
| `SSC-SST-FR-002` | The crate shall implement the three traits for `u8`, `u16`, `u32`, `u64`, `u128`, `usize`, `i8`, `i16`, `i32`, `i64`, `i128`, `isize`, `f32` and `f64`, as today. | 0.1 |
| `SSC-SST-FR-003` | The crate shall build with `cargo build --target wasm32-unknown-unknown`, with default features and with the `derive` feature. | 0.1 |
| `SSC-SST-FR-004` | `rust_serial_file` shall depend on the new crate and expose it as `rust_serial_file::serialized_struct` (`pub use serialized_struct;`), so the trait paths used today stay valid. | 0.1 |
| `SSC-SST-FR-005` | `rust_serial_file/src/serialized_struct/` shall be removed; there shall be a single definition of each trait in the workspace, so a type implementing the traits through one path is accepted by APIs using the other path. | 0.1 |
| `SSC-SST-FR-006` | An optional cargo feature `derive` shall re-export the `SerializedStruct` derive macro from `serialized_struct_derive`, so a browser project can write `use serialized_struct::SerializedStruct;` and get both the trait and the derive. | 0.1 |
| `SSC-SST-FR-007` | A struct deriving `SerializedStruct` shall compile when the user imports the traits through either `serialized_struct` (new crate) or `rust_serial_file::serialized_struct` (re-export). | 0.1 |
| `SSC-SST-FR-008` | The crate shall provide `BlobRef { offset: u64, len: u32 }`, a 12-byte `SerializedStruct` (offset then length, little-endian) used as a record field pointing into a blob file (`rust_serial_file/doc/requirements/file/dynamic_storage.md`). `BlobRef::EMPTY` (`offset = 0`, `len = 0`) means an empty value with no storage. It holds no I/O, so records using it still build for WebAssembly. | 0.2 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `SSC-SST-DS-001` | Encoded bytes for every primitive and every derived struct shall be identical before and after the split (little-endian, fields in declaration order, no padding). | 0.1 |
| `SSC-SST-DS-002` | `usize` and `isize` are 4 bytes on `wasm32` and 8 bytes on 64-bit hosts. Fields of these types in a struct using the derive are always stored as 64-bit values (`PSF-SSD-FR-001` to `PSF-SSD-FR-006`), so derived structs have the same layout on both sides. The primitive implementations used directly (for example `StructFile<usize>`) keep the native width, and the crate documentation shall say so. | 0.2 |
| `SSC-SST-DS-003` | Checked methods `try_encode`/`try_decode` shall keep reporting buffers shorter than `SIZE` as errors, as specified in `encode_decode_raw.md`. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `SSC-SST-RS-001` | The crate shall not use `unsafe` code. | 0.1 |
| `SSC-SST-RS-002` | The crate's dependency tree (`cargo tree -p serialized_struct --target wasm32-unknown-unknown -e normal`) shall contain neither `rand` nor `getrandom` nor `utils`. | 0.1 |
| `SSC-SST-RS-003` | The derive macro keeps emitting the relative path `serialized_struct::…`. This path resolves to the new crate when it is a dependency, and to the re-exported module when the user has `use rust_serial_file::serialized_struct;`. The macro shall not switch to `::serialized_struct::…`, which would break users that only depend on `rust_serial_file`. | 0.1 |
| `SSC-SST-RS-004` | Edition 2024, same as `rust_serial_file`. | 0.1 |

## Public API

```rust
// serialized_struct/src/lib.rs
pub trait EncodeRaw { fn encode(&self, data: &mut [u8]) -> usize; }

pub trait DecodeRaw {
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self;
    fn from_le_bytes_only(data: &[u8]) -> Self where Self: Sized;
}

pub trait SerializedStruct: DecodeRaw + EncodeRaw {
    const SIZE: usize;
    fn try_encode(&self, data: &mut [u8]) -> anyhow::Result<usize> where Self: Sized;
    fn try_decode(data: &[u8]) -> anyhow::Result<Self> where Self: Sized;
}

#[cfg(feature = "derive")]
pub use serialized_struct_derive::SerializedStruct;

// rust_serial_file/src/lib.rs
pub use serialized_struct;
```

Browser usage:

```toml
[dependencies]
serialized_struct = { path = "../serialized_struct", features = ["derive"] }
```

```rust
use serialized_struct::{SerializedStruct, DecodeRaw, EncodeRaw};

#[derive(SerializedStruct)]
pub struct Sample { pub time: u64, pub value: f64 }
```

## Errors and failure behavior

- Unchanged from `encode_decode_raw.md`: the raw methods panic on short
  slices (documented behavior), the checked methods return `anyhow::Error`.
- No new failure mode is introduced by the split.

## Safety and security

- None: no `unsafe` code is allowed (`SSC-SST-RS-001`).
- Bytes received from the browser are untrusted. Decoding a primitive accepts
  any bit pattern, so decoding cannot cause undefined behavior; callers must
  use `try_decode` (or check length) on untrusted buffers to avoid panics.

## Compatibility

- Source compatibility: `rust_serial_file::serialized_struct::{EncodeRaw,
  DecodeRaw, SerializedStruct}` and the module path used with
  `use rust_serial_file::serialized_struct;` keep working (`SSC-SST-FR-004`).
- `rust_python_file` keeps compiling without change.
- Data compatibility: byte format unchanged (`SSC-SST-DS-001`).
- Migration for downstream projects (optional): replace
  `rust_serial_file::serialized_struct` by `serialized_struct` to drop the
  dependency on `rust_serial_file` in browser code.

## Performance and resource use

- No change: the code is moved, not rewritten. Non-goal: any performance
  improvement.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Create the crate:** add `serialized_struct/` (`Cargo.toml`, `src/lib.rs`) with the content of the current module, move `tests/serialized_struct_raw.rs` into it, add the `derive` feature. | None | `cargo test -p serialized_struct` passes; the crate has no dependency other than `anyhow` (and optional derive). |
| P-002 | **Switch `rust_serial_file`:** add the dependency, replace `mod serialized_struct` by `pub use serialized_struct;`, delete the old module, fix internal `crate::serialized_struct` paths. | P-001 | `rust_serial_file` and `rust_python_file` build and all their tests pass unchanged. |
| P-003 | **WebAssembly check and `BlobRef`:** add `BlobRef`; build the crate (with and without `derive`) for `wasm32-unknown-unknown`, check `cargo tree` for forbidden crates, add a test deriving a struct through each import path. | P-002 | `SSC-SST-FR-003`, `SSC-SST-FR-007`, `SSC-SST-FR-008` and `SSC-SST-RS-002` verified (T-006, T-008). T-009 is run with the derive change (`pointer_sized_fields.md` P-002). |
| P-004 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-003 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-005 | **Documentation:** update this document, move `encode_decode_raw.md` into the new crate, add crate-level docs (WebAssembly usage and the `usize` warning). | P-004 | The document matches the implemented behavior and test results; public items have up-to-date doc comments. |

### Current implementation checklist

- [x] P-001 — `Create the serialized_struct crate` — Done
- [x] P-002 — `Re-export from rust_serial_file` — Done
- [x] P-003 — `WebAssembly build check and BlobRef` — Done
- [x] P-004 — `Coverage of touched files` — Done
- [x] P-005 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- The work is a move plus a re-export; splitting it into "create", "switch"
  and "verify on WASM" lets each step be built and tested on its own and
  keeps `rust_serial_file` green between steps.
- P-002 depends on P-001 only. P-003 needs both because the import-path test
  uses the re-export.
- Risk: the derive macro's relative path (`SSC-SST-RS-003`). If a user has
  both the crate and the re-export in scope under the same name with
  different items, resolution breaks; FR-005 (single definition) removes that
  risk. Risk: installing the WASM target may need network access in CI.
- Confirmed: the crate name `serialized_struct` (chosen because it is
  exactly the path the derive macro already emits).

**Implementation reflection (update as work progresses):**

- The plan held. The code moved with `git mv` and only gained doc comments;
  no internal path changed in `rust_serial_file`, because
  `crate::serialized_struct` resolves through the `pub use` re-export.
- `encode_decode_raw.md` moved to `serialized_struct/doc/requirements/` with
  its IDs unchanged.
- `BlobRef` implements the traits by hand: the derive emits
  `serialized_struct::…`, which does not resolve inside the crate itself.
- P-003 criteria changed: T-009 checks the 64-bit layout of derived
  `usize`/`isize` fields, which only exists after the derive change. It stays
  ⏳ Not run here and is run with `pointer_sized_fields.md` P-002.
- `rust_serial_file` still pulls `rand` through `utils`; this is removed by
  `utils/doc/requirements/performance/random_injection.md`, not here. The
  browser client is unblocked anyway, since it no longer needs
  `rust_serial_file`.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `SSC-SST-FR-001`, `SSC-SST-FR-002`, `SSC-SST-DS-001`, `SSC-SST-DS-003` | Existing primitive round-trip and checked encode/decode tests, moved to the new crate. | All pass unchanged; encoded bytes equal the `to_le_bytes` of each value. |
| T-002 | `SSC-SST-FR-006`, `SSC-SST-FR-007`, `SSC-SST-RS-003` | Derive a struct using `serialized_struct::SerializedStruct` from the new crate with the `derive` feature. | Compiles; `SIZE` equals the sum of field sizes; round trip returns the same value. |
| T-003 | `SSC-SST-FR-004`, `SSC-SST-FR-007` | In `rust_serial_file`, derive a struct with `use rust_serial_file::serialized_struct;` and store it in a `StructFile`. | Compiles and round-trips through the file. |
| T-004 | `SSC-SST-FR-005` | A type implementing the traits from the new crate is accepted by `StructFile<T>`. | Compiles (single trait definition). |
| T-005 | `SSC-SST-DS-001` | Golden bytes: encode a fixed derived struct and compare with a byte array captured before the split. | Bytes are equal. |
| T-006 | `SSC-SST-FR-003`, `SSC-SST-RS-002` | `cargo build -p serialized_struct --target wasm32-unknown-unknown` (default and `--features derive`) and `cargo tree` check. | Both builds succeed; `rand`, `getrandom`, `utils` absent from the tree. |
| T-007 | `SSC-SST-FR-004` | Build and test `rust_serial_file` and `rust_python_file` (`cargo test`). | All existing tests pass without source changes in tests other than moved files. |
| T-008 | `SSC-SST-FR-008` | `BlobRef` round trip and golden bytes; `BlobRef::EMPTY.is_empty()`. | `SIZE == 12`; bytes are offset then len, little-endian. |
| T-009 | `SSC-SST-DS-002` | Derived struct with a `usize` field built for `wasm32-unknown-unknown` with a `const` assertion on `SIZE`. | Same `SIZE` as on the host. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `serialized_struct/tests/serialized_struct_raw.rs` | existing test functions (moved) |
| T-002 | `serialized_struct/tests/derive.rs` | `derives_through_new_crate` |
| T-003 | `rust_serial_file/tests/struct_file.rs` | `derives_through_reexport` |
| T-004 | `rust_serial_file/tests/struct_file.rs` | `accepts_type_from_new_crate` |
| T-005 | `serialized_struct/tests/derive.rs` | `derived_layout_matches_golden_bytes` |
| T-006 | Shell check (documented command) | `wasm32 build + cargo tree` |
| T-007 | Workspace (`cargo test` in each crate) | all |
| T-008 | `serialized_struct/tests/blob_ref.rs` | `blob_ref_layout` |
| T-009 | `serialized_struct/tests/derive.rs` | `pointer_sized_layout_is_target_independent` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-25 | `daba46e` | ✅ Success |
| T-002 | 2026-11-25 | `daba46e` | ✅ Success |
| T-003 | 2026-11-25 | `daba46e` | ✅ Success |
| T-004 | 2026-11-25 | `daba46e` | ✅ Success |
| T-005 | 2026-11-25 | `daba46e` | ✅ Success |
| T-006 | 2026-11-25 | `daba46e` | ✅ Success |
| T-007 | 2026-11-25 | `daba46e` | ⚠️ Mitigate |
| T-008 | 2026-11-25 | `daba46e` | ✅ Success |
| T-009 | — | — | ⏳ Not run |

#### Notes

- Commands: `cargo test --all-features` in `serialized_struct/` (T-001,
  T-002, T-005, T-008; T-002 also runs `tests/derive_feature.rs` with the
  `derive` feature), `cargo test` in `rust_serial_file/` (T-003, T-004,
  T-007: all 202 tests, including doc tests, pass), `cargo fmt --check` and
  `cargo clippy --all-targets --all-features` clean in both crates.
- T-005: the golden bytes are written out by hand from the little-endian
  rules; the encoder code is the pre-split code moved unchanged, so the
  bytes are those produced before the split.
- T-006: `cargo build --target wasm32-unknown-unknown` with and without
  `--features derive`, plus `cargo build --tests --features derive --target
  wasm32-unknown-unknown` (derived structs and `BlobRef` compile for wasm32).
  `cargo tree --target wasm32-unknown-unknown -e normal --features derive`
  lists only `anyhow` and `serialized_struct_derive` (and its proc-macro
  dependencies).
- T-009: not run yet; see the implementation reflection.

#### Warning notes

- **T-007:** `rust_python_file` builds (`cargo build`), but `cargo test`
  fails to link its test binary. The same failure happens on the code before
  this change: the `pyo3` `extension-module` feature does not link libpython,
  so test executables have unresolved Python symbols. Impact: none on these
  requirements (the crate compiles against the re-export). Possible
  resolution: make `extension-module` an optional feature enabled only by
  `maturin`, so `cargo test` links libpython; to be decided by Olographik.

### Acceptance criteria

1. The browser project builds for `wasm32-unknown-unknown` depending only on
   `serialized_struct` (and optionally the derive).
2. Byte format is unchanged (golden test).
3. Existing users of `rust_serial_file::serialized_struct` compile unchanged.
4. `cargo fmt`, `cargo clippy`, build and tests pass for every touched crate.

## Code coverage

- **Coverage summary:** `serialized_struct/src/lib.rs` is fully covered. `rust_serial_file/src/lib.rs` was touched but has no executable code left (module declarations and a re-export).

### Current coverage results

- **Tool and command:** `cargo llvm-cov --all-features --summary-only` (cargo-llvm-cov 0.9.2) in `serialized_struct/`
- **Date of run:** 2026-11-25
- **Git hash:** `daba46e`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `serialized_struct/src/lib.rs` | 100.00% (9/9) | 100.00% (52/52) | 100.00% (69/69) | N/A |

### Coverage notes

- None. Branch coverage is `N/A`: it needs a nightly toolchain
  (`--branch`), and the file has no reported branches.

## Out of scope

- `no_std` support (would allow embedded targets); possible later with
  `anyhow` default features off. Decision: owner of the repository.
- Variable-length (dynamic) data inside `SerializedStruct` itself; only the
  `BlobRef` handle lives here, the storage is in `rust_serial_file`.
- Changing the width of the primitive `usize`/`isize` implementations used
  outside the derive. Decision: Olographik.
- A Cargo workspace root (`Cargo.toml` with `[workspace]`) for the repository;
  the crates stay independent as today.
