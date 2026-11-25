# Pointer-Sized Fields Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.3
- **Short description:** Make the derive macros reject `usize` and `isize` struct fields with a clear compile error, because their size depends on the target and serialized records must have the same layout on every platform.

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

`serialized_struct_derive` provides `#[derive(SerializedStruct)]`,
`#[derive(EncodeRaw)]` and `#[derive(DecodeRaw)]`. Each field is encoded
through its own `EncodeRaw`/`DecodeRaw` implementation and `SIZE` is the sum of
the fields' `SIZE`. `usize` and `isize` have the target's pointer width:
4 bytes on `wasm32`, 8 bytes on 64-bit hosts. A record using them would have a
different `SIZE` and layout on the WebAssembly browser client and on the host
server, which defeats the purpose of these records: a platform-independent
byte format.

Revision 0.1 planned to widen such fields to 64 bits. After reflection, the
owner chose to reject them instead: the derive reports a compile error on each
`usize`/`isize` field, telling the user to use a fixed-width type or to write
the encoding by hand. The `serialized_struct` crate also stops implementing the
traits for `usize` and `isize` (`RSF-DS-004` in `encode_decode_raw.md`,
`SSC-SST-FR-002` in `serialized_struct_crate.md`), so every way of encoding
them fails at compile time.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-25 | Olographik | Planning reflection: panic in the raw decoder confirmed by the owner; clarified that `try_decode` is the path used by `StructFile`. No requirement changed. |
| 0.3 | 2026-11-25 | Olographik | Design changed from widening `usize`/`isize` fields to rejecting them at compile time. Modified `PSF-SSD-FR-001` to `PSF-SSD-FR-004`, `PSF-SSD-DS-001`, `PSF-SSD-DS-002`, `PSF-SSD-RS-001`, `PSF-SSD-RS-002`. Removed `PSF-SSD-FR-006` (range-checked `try_decode`) and `PSF-SSD-DS-003` (raw decode panic), which no longer apply. Tests T-002 to T-006 redefined, T-007 added. Phases P-002 redefined. Implemented, with results, coverage and reflection. |

## Goals and scope

### Goals

- A struct with a `usize` or `isize` field does not compile with any of the
  three derives.
- The compile error points at the field and says what to do instead.
- Structs without such fields expand exactly as before.

### Scope

- `serialized_struct_derive/src/lib.rs`: the three derives, for named and
  tuple structs.
- Related change in another module (own documents): removal of the
  `usize`/`isize` trait implementations in `serialized_struct`.
- Unchanged: code generated for every other field type.

## Assumptions and dependencies

- Detection is syntactic (the proc macro sees tokens, not resolved types).
  Anything it cannot recognize by name still fails, because the
  `serialized_struct` crate has no `usize`/`isize` implementations.
- Dependencies unchanged: `syn` 2, `quote`, `proc-macro2`. The end-to-end
  compile-fail tests use `trybuild` as a dev-dependency of `serialized_struct`.
- The generated code refers to `serialized_struct::…` as before (see
  `SSC-SST-RS-003` in `serialized_struct/doc/requirements/serialized_struct_crate.md`).

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-FR-001` | A field whose type is the path `usize` (also written `core::primitive::usize`, `std::primitive::usize`, `::core::primitive::usize`, `::std::primitive::usize`, or wrapped in parentheses or an invisible macro group) shall make the derive emit a compile error. | 0.3 |
| `PSF-SSD-FR-002` | The same applies to `isize`, with the same accepted spellings. | 0.3 |
| `PSF-SSD-FR-003` | The error shall be attached to the field's type and say which derive, which primitive, which field (name, or index for tuple structs) and which struct, and suggest a fixed-width type (for example `u64` or `i64`) or a hand-written `EncodeRaw`, `DecodeRaw` and `SerializedStruct` implementation. | 0.3 |
| `PSF-SSD-FR-004` | One error shall be reported per offending field, and no trait implementation shall be generated for a struct with such a field. | 0.3 |
| `PSF-SSD-FR-005` | The rule applies to the three derives (`SerializedStruct`, `EncodeRaw`, `DecodeRaw`) and to named and tuple structs. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-DS-001` | A derived record never contains a field whose width depends on the target, so its `SIZE` and byte layout are the same on every platform. | 0.3 |
| `PSF-SSD-DS-002` | For structs without `usize`/`isize` fields, the generated code, `SIZE` and bytes are unchanged. | 0.3 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-RS-001` | Type aliases (`type Index = usize;`), `usize` inside other types (`Option<usize>`, arrays, generics) and nested structs are not recognized by name. They still fail to compile, with the compiler's "trait not implemented" error, because `usize`/`isize` do not implement the traits. The crate documentation shall say so. | 0.3 |
| `PSF-SSD-RS-002` | No `unsafe` code. Each derive's expansion shall be a plain function over `syn::DeriveInput`, so it can be unit tested; the `#[proc_macro_derive]` functions only parse and delegate. | 0.3 |
| `PSF-SSD-RS-003` | Edition and dependencies of `serialized_struct_derive` unchanged. | 0.1 |

## Public API

No new public item. A struct such as

```rust
#[derive(SerializedStruct)]
struct Row { time: u64, index: usize }
```

fails to compile with:

```text
error: #[derive(SerializedStruct)] does not support the `usize` field `index` of `Row`: its size depends on the target platform. Use a fixed-width type (for example `u64` or `i64`), or implement `EncodeRaw`, `DecodeRaw` and `SerializedStruct` by hand for this struct.
 --> src/main.rs:4:12
  |
4 |     index: usize,
  |            ^^^^^
```

A user who needs `usize` in memory implements the three traits on the record
by hand and chooses the stored width (for example `u64`, with a checked
conversion on decode).

## Errors and failure behavior

- Failures are compile errors only; nothing changes at run time.
- Enums and unions still make the derive panic with "supports only structs"
  (unchanged behavior; the compiler reports it as a derive error).

## Safety and security

- None: no `unsafe` code is generated or used.

## Compatibility

- Breaking for any downstream record that derives the traits with a
  `usize`/`isize` field: it no longer compiles and must switch to a
  fixed-width type or a hand-written implementation. None exists in this
  workspace.
- Data compatibility: unaffected for every record that still compiles
  (`PSF-SSD-DS-002`).

## Performance and resource use

- No run-time effect. The extra check is a scan of the field types at
  compile time.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Detection helper:** function recognizing the accepted `usize`/`isize` paths in a `syn::Type`. | None | Unit tests of the helper pass (T-001). |
| P-002 | **Compile errors:** emit one spanned error per offending field in each derive, move expansions into testable functions, add `trybuild` and `compile_fail` tests. | P-001, and the `serialized_struct` crate extraction. | T-002 to T-007 pass. |
| P-003 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001, P-002 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-004 | **Documentation:** update this requirements document and the code comments of the touched files (derive docs list what is and is not detected). | P-003 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [x] P-001 — `Pointer-sized type detection` — Done
- [x] P-002 — `Compile errors for pointer-sized fields` — Done
- [x] P-003 — `Coverage of touched files` — Done
- [x] P-004 — `Documentation update` — Done

**Planning reflection (complete before implementation):**

- Detection is isolated so it can be tested without expanding code.
- Revision 0.3: rejecting is simpler and safer than widening. No run-time
  range check or panic is needed, and the layout question cannot come back
  through `StructFile<usize>`, since the primitive implementations are gone
  too.

**Implementation reflection (update as work progresses):**

- Implemented on 2026-11-25. P-002 changed from "widen to 64 bits" to
  "reject" before any widening code was written; no phase was added.
- The `trybuild` test for a type alias was dropped: its expected output was
  the compiler's own trait-error text, which changes between Rust versions.
  The alias case is covered by a `compile_fail` doc test in
  `serialized_struct/src/lib.rs` instead, which only requires that it fails.
- To make the generators testable, each derive's expansion moved into
  `expand_encode_raw`, `expand_decode_raw` and `expand_serialized_struct`
  (`PSF-SSD-RS-002`). The generated code itself did not change.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `PSF-SSD-FR-001`, `PSF-SSD-FR-002`, `PSF-SSD-RS-001` | Detection helper on `usize`, `isize`, `core::primitive::usize`, `::std::primitive::isize`, `(usize)`, a grouped `isize`, and on `u64`, `i32`, `Option<usize>`, `[usize; 2]`, alias `Index`, `::usize`, `other::primitive::usize`, `core::usize`, `<T as Trait>::usize`. | The first six are detected, the others are not. |
| T-002 | `PSF-SSD-FR-001`, `PSF-SSD-FR-003`, `PSF-SSD-FR-005` | `trybuild`: `#[derive(SerializedStruct)]` on a named struct with a `usize` field. | Fails with the expected message, pointing at the field type. |
| T-003 | `PSF-SSD-FR-002`, `PSF-SSD-FR-003`, `PSF-SSD-FR-005` | `trybuild`: tuple struct with a `core::primitive::isize` field. | Fails with the field index `1` in the message. |
| T-004 | `PSF-SSD-FR-004`, `PSF-SSD-FR-005` | `trybuild`: `#[derive(EncodeRaw)]` and `#[derive(DecodeRaw)]` alone on structs with `usize` / `isize` fields; unit test with two offending fields. | One error per field, naming the derive used; no implementation generated. |
| T-005 | `PSF-SSD-RS-001`, `PSF-SSD-DS-001` | `compile_fail` doc test: field typed with an alias of `usize`. | Does not compile. |
| T-006 | `PSF-SSD-DS-002`, `PSF-SSD-RS-002` | Unit tests of the expansions on named, tuple and unit structs, and on an enum and a union. | Generated code as before; enums/unions panic with "supports only structs". |
| T-007 | `PSF-SSD-DS-001`, `PSF-SSD-DS-002` | Existing derive users still compile and pass: `serialized_struct`, `rust_serial_file`, `rust_python_file` tests; `cargo build --tests --features derive --target wasm32-unknown-unknown` in `serialized_struct`; clippy. | All pass; no new warning. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `serialized_struct_derive/src/lib.rs` (unit tests) | `detects_pointer_sized_types` |
| T-002 | `serialized_struct/tests/compile_fail.rs` + `tests/ui/usize_field.rs` | `pointer_sized_fields_are_rejected` |
| T-003 | `serialized_struct/tests/compile_fail.rs` + `tests/ui/isize_tuple_field.rs` | `pointer_sized_fields_are_rejected` |
| T-004 | `serialized_struct/tests/compile_fail.rs` + `tests/ui/raw_derives_usize.rs`; `serialized_struct_derive/src/lib.rs` | `pointer_sized_fields_are_rejected`, `rejects_only_structs_with_pointer_sized_fields`, `expansions_report_pointer_sized_fields` |
| T-005 | `serialized_struct/src/lib.rs` (crate doc test) | `compile_fail` block on `type Index = usize` |
| T-006 | `serialized_struct_derive/src/lib.rs` (unit tests) | `expands_named_tuple_and_unit_structs`, `serialized_struct_rejects_enums`, `encode_raw_rejects_enums`, `decode_raw_rejects_unions` |
| T-007 | Workspace (`cargo test` in each crate) and shell check | all |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-002 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-003 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-004 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-005 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-006 | 2026-11-25 | `6fc4c4d` | ✅ Success |
| T-007 | 2026-11-25 | `6fc4c4d` | ✅ Success |

#### Notes

- Commands: `cargo test` in `serialized_struct_derive/` (7 unit tests);
  `cargo test --all-features` in `serialized_struct/` (16 tests, including
  the `trybuild` run and 3 `compile_fail` doc tests); `cargo test` in
  `rust_serial_file/` (202) and `rust_python_file/` (7); `cargo clippy
  --all-targets` clean in the touched crates (the one `rust_python_file`
  warning, `needless_range_loop` in `src/file/mod.rs`, predates this work).
- The expected compiler output is stored in `serialized_struct/tests/ui/*.stderr`;
  regenerate with `TRYBUILD=overwrite cargo test --features derive --test compile_fail`
  after an intended message change.

#### Warning notes

- None.

### Acceptance criteria

1. A derived struct with a `usize`/`isize` field does not compile, and the
   error says why and what to do.
2. Structs without such fields are unchanged.
3. `cargo fmt`, `cargo clippy`, build and tests pass.

## Code coverage

- **Coverage summary:** 96.28% of lines. The only uncovered code is the three
  `#[proc_macro_derive]` entry points.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --summary-only` (cargo-llvm-cov 0.9.2) in `serialized_struct_derive/`
- **Date of run:** 2026-11-25
- **Git hash:** `6fc4c4d`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `serialized_struct_derive/src/lib.rs` | 90.62% (29/32) | 96.28% (233/242) | 96.69% (438/453) | N/A |

### Coverage notes

- **`serialized_struct_derive/src/lib.rs`:** `derive_encode_raw`,
  `derive_decode_raw` and `derive_serialized_struct` (3 functions, 9 lines)
  are not covered. They take a `proc_macro::TokenStream`, which only exists
  while the compiler runs a macro, so unit tests cannot call them; when the
  compiler runs them for other crates' tests, coverage is not recorded. They
  only parse the input and call the `expand_*` functions, which are covered.
  Branch coverage is `N/A` (needs a nightly toolchain).

## Out of scope

- Widening `usize`/`isize` to 64 bits automatically (the revision 0.1 design),
  rejected in revision 0.3.
- A field attribute to choose a stored width for a `usize` field (for example
  `#[serialized(as = "u64")]`). Could be added later if hand-written
  implementations become common. Decision: Olographik.
