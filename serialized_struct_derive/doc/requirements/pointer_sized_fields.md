# Pointer-Sized Fields Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Make the derive macros encode and decode `usize` and `isize` struct fields as 64-bit little-endian values, so a derived struct has the same size and byte layout on `wasm32` and on 64-bit hosts.

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
the fields' `SIZE`. For `usize` and `isize` this is the target's pointer width:
4 bytes on `wasm32`, 8 bytes on 64-bit hosts. A struct shared between the
WebAssembly browser client and the host server therefore has a different
`SIZE` and layout on each side, and the bytes cannot be exchanged.

This document requires the derive macros to detect `usize` and `isize` fields
and always encode them as `u64` / `i64` (8 bytes, little-endian), and decode
them back with a range check.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|

## Goals and scope

### Goals

- A derived struct has the same `SIZE` and byte layout on every target,
  including when it has `usize`/`isize` fields.
- Files already written on 64-bit hosts are read unchanged (they already use
  8 bytes for these fields).
- A value that does not fit the decoding target's `usize`/`isize` is never
  silently truncated.

### Scope

- `serialized_struct_derive/src/lib.rs`: the three derives, for named and
  tuple structs.
- Unchanged: the primitive `impl` of the traits for `usize`/`isize` in the
  `serialized_struct` crate (still native width when used directly, for
  example `StructFile<usize>`); every other field type.

## Assumptions and dependencies

- Supported targets have a pointer width of at most 64 bits, so `usize → u64`
  and `isize → i64` are always lossless on encode.
- Detection is syntactic (the proc macro sees tokens, not resolved types).
- Dependencies unchanged: `syn` 2, `quote`, `proc-macro2`.
- The generated code refers to `serialized_struct::…` as today (see
  `SSC-SST-RS-003` in `serialized_struct/doc/requirements/serialized_struct_crate.md`).

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-FR-001` | A field whose type is the path `usize` (also written `core::primitive::usize`, `std::primitive::usize`, `::core::primitive::usize`, `::std::primitive::usize`) shall be encoded as `(value as u64)` with the `u64` encoder. | 0.1 |
| `PSF-SSD-FR-002` | A field whose type is the path `isize` (same accepted spellings) shall be encoded as `(value as i64)` with the `i64` encoder. | 0.1 |
| `PSF-SSD-FR-003` | Such fields shall be decoded by reading a `u64` / `i64` and converting with `usize::try_from` / `isize::try_from`. | 0.1 |
| `PSF-SSD-FR-004` | `#[derive(SerializedStruct)]` shall count 8 bytes for each such field in `SIZE` (`<u64 as SerializedStruct>::SIZE` / `<i64 as …>::SIZE`). | 0.1 |
| `PSF-SSD-FR-005` | The rule applies to the three derives (`SerializedStruct`, `EncodeRaw`, `DecodeRaw`) and to named and tuple structs. | 0.1 |
| `PSF-SSD-FR-006` | `#[derive(SerializedStruct)]` shall generate a `try_decode` override that returns an error when a pointer-sized field's stored value does not fit the target's `usize`/`isize`, in addition to the existing length check. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-DS-001` | Pointer-sized fields occupy exactly 8 bytes, little-endian, at their declaration position, on every target. | 0.1 |
| `PSF-SSD-DS-002` | On 64-bit targets the bytes are identical to the current output, so existing files stay readable. | 0.1 |
| `PSF-SSD-DS-003` | The raw decoder (`from_le_bytes_slices`), which cannot return an error, shall panic with a message naming the struct and field when a value does not fit, consistent with the raw API panicking on short slices. Callers decoding untrusted bytes use `try_decode`. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `PSF-SSD-RS-001` | Type aliases (`type Index = usize;`), `usize` inside other types (`Option<usize>`, arrays, generics) and nested structs are not detected; they keep their own implementation. The derive documentation shall say so. A nested struct that itself derives `SerializedStruct` is handled by its own derive. | 0.1 |
| `PSF-SSD-RS-002` | The generated code shall not use `unsafe` and shall compile without warnings (no `clippy::cast_possible_truncation` on the encode cast, which is widening). | 0.1 |
| `PSF-SSD-RS-003` | Edition and dependencies of `serialized_struct_derive` unchanged. | 0.1 |

## Public API

No new public item. Generated code, for

```rust
#[derive(SerializedStruct)]
struct Row { time: u64, index: usize }
```

becomes, in essence:

```rust
// encode
offset += self.time.encode(&mut data[offset..]);
offset += (self.index as u64).encode(&mut data[offset..]);

// decode
index: {
    let raw = <u64 as serialized_struct::DecodeRaw>::from_le_bytes_slices(bytes, offset);
    usize::try_from(raw).expect("Row.index: stored value does not fit in usize")
},

// size
const SIZE: usize = <u64 as SerializedStruct>::SIZE + <u64 as SerializedStruct>::SIZE;
```

## Errors and failure behavior

- Encode never fails for pointer-sized fields (widening conversion).
- `try_decode`: returns an error naming the field when the value does not fit
  (`PSF-SSD-FR-006`). Only possible on targets narrower than 64 bits.
- Raw decode: panics with the same message (`PSF-SSD-DS-003`).

## Safety and security

- None: no `unsafe` code is generated.
- Bytes from the browser are untrusted: the range check prevents an
  out-of-range value from being truncated into a wrong index.

## Compatibility

- 64-bit hosts: byte-identical, no migration.
- `wasm32` (and any 32-bit target): `SIZE` and layout change for structs with
  `usize`/`isize` fields. Data written on a 32-bit target with the old derive
  cannot be read with the new one; no such persisted data is known (the
  browser only exchanges messages).
- Source compatible: field types and user code unchanged.

## Performance and resource use

- One widening cast per field on encode, one range check on decode;
  negligible.
- On 32-bit targets, records with pointer-sized fields grow by 4 bytes per
  field. Accepted as the cost of a stable layout.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Detection helper:** function recognizing the accepted `usize`/`isize` paths in a `syn::Type`. | None | Unit tests of the helper pass (T-001). |
| P-002 | **Code generation:** use the helper in encode, decode, `SIZE` and the new `try_decode` override. | P-001, and the `serialized_struct` crate extraction (P-002 of `serialized_struct_crate.md`) so tests use the new crate. | T-002 to T-006 pass. |
| P-003 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001, P-002 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-004 | **Documentation:** update this requirements document and the code comments of the touched files (derive docs list what is and is not detected). | P-003 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [ ] P-001 — `Pointer-sized type detection` — Not started
- [ ] P-002 — `Generated encode, decode, SIZE and try_decode` — Not started
- [ ] P-003 — `Coverage of touched files` — Not started
- [ ] P-004 — `Documentation update` — Not started

**Planning reflection (complete before implementation):**

- Detection is isolated so it can be tested without expanding code.
- Risk: proc-macro tests run on the host (64-bit), so the "does not fit"
  branch cannot be reached by decoding on the host. The 32-bit check is done
  by building for `wasm32-unknown-unknown` and running the range-check logic
  through `try_decode` on a crafted buffer with a value above `u32::MAX`
  under a 32-bit target, or by testing the generated conversion against
  `u32` in a unit test of the emitted pattern.
- Decision to confirm: panic in the raw decoder (`PSF-SSD-DS-003`) instead of
  saturating.

**Implementation reflection (update as work progresses):**

- None yet.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `PSF-SSD-FR-001`, `PSF-SSD-FR-002`, `PSF-SSD-RS-001` | Detection helper on `usize`, `isize`, `core::primitive::usize`, `::std::primitive::isize`, `u64`, `Option<usize>`, alias path `Index`. | First four detected, last three not. |
| T-002 | `PSF-SSD-FR-004`, `PSF-SSD-DS-001` | `SIZE` of `struct A { a: u8, b: usize, c: isize }`. | `SIZE == 17` on every target (checked on host and in the wasm build with a `const` assertion). |
| T-003 | `PSF-SSD-FR-001`, `PSF-SSD-FR-002`, `PSF-SSD-FR-003`, `PSF-SSD-DS-002` | Round trip and golden bytes for named and tuple structs with `usize`/`isize` fields (including `isize::MIN`, `-1`). | Bytes equal `u64`/`i64` little-endian encodings; decode returns the original. |
| T-004 | `PSF-SSD-FR-005` | `#[derive(EncodeRaw, DecodeRaw)]` alone on a struct with a `usize` field. | Same bytes as T-003. |
| T-005 | `PSF-SSD-FR-006`, `PSF-SSD-DS-003` | Value `u64::MAX` decoded into a `usize` field on a 32-bit target (wasm build test or simulated through the emitted conversion). | `try_decode` errors naming the field; raw decode panics with that message. |
| T-006 | `PSF-SSD-RS-002` | `cargo clippy --all-targets -- -D warnings` on a crate using the derive, and `cargo build --target wasm32-unknown-unknown` of the `serialized_struct` derive test. | No warnings; build succeeds. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `serialized_struct_derive/src/lib.rs` (unit tests) | `detects_pointer_sized_types` |
| T-002 | `serialized_struct/tests/derive.rs` | `pointer_sized_fields_are_8_bytes` |
| T-003 | `serialized_struct/tests/derive.rs` | `pointer_sized_round_trip_and_bytes` |
| T-004 | `serialized_struct/tests/derive.rs` | `separate_derives_widen_pointer_sized` |
| T-005 | `serialized_struct/tests/derive.rs` | `pointer_sized_out_of_range` |
| T-006 | Shell check (documented command) | `clippy + wasm32 build` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | — | — | ⏳ Not run |
| T-002 | — | — | ⏳ Not run |
| T-003 | — | — | ⏳ Not run |
| T-004 | — | — | ⏳ Not run |
| T-005 | — | — | ⏳ Not run |
| T-006 | — | — | ⏳ Not run |

#### Notes

- None.

#### Warning notes

- None.

### Acceptance criteria

1. A derived struct with `usize`/`isize` fields has the same `SIZE` and bytes
   on `wasm32` and on x86_64.
2. Existing 64-bit data is read unchanged.
3. Out-of-range values are reported, never truncated.
4. `cargo fmt`, `cargo clippy`, build and tests pass.

## Code coverage

- **Coverage summary:** Not run yet.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --summary-only` (planned)
- **Date of run:** —
- **Git hash:** —

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `serialized_struct_derive/src/lib.rs` | — | — | — | — |

### Coverage notes

- Not run yet. Proc-macro code runs at compile time; coverage of it is
  measured through the helper's unit tests.

## Out of scope

- Changing the width of the primitive `usize`/`isize` implementations used
  outside the derive (for example `StructFile<usize>`). Decision: Olographik.
- Detecting `usize` through type aliases or inside generic types.
- A field attribute to opt out (keep native width). Can be added if needed.
