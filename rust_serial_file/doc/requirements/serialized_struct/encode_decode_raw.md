# Raw Encode and Decode Requirements

- **Date:** 2026-11-08
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Expose raw little-endian `EncodeRaw` and `DecodeRaw` traits and provide opt-in checked encode/decode methods for fixed-layout serialized values.

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

Specify the public raw serialization traits declared by
`rust_serial_file/src/serialized_struct/mod.rs`. `EncodeRaw` writes a value's
little-endian representation into a caller-provided byte slice.
`DecodeRaw` reconstructs a value from a byte slice and supports advancing a
caller-owned offset when decoding sequential fields.

The public crate module shall expose these traits as
`rust_serial_file::serialized_struct::EncodeRaw` and
`rust_serial_file::serialized_struct::DecodeRaw`. The module also contains
`SerializedStruct`, which composes both traits and provides a fixed-size
constant used by other serialization components.

## Goals and scope

### Goals

- Provide a stable, documented public path to `EncodeRaw` and `DecodeRaw`.
- Define little-endian encoding and decoding semantics for the primitive
  implementations.
- Preserve offset-based decoding so structured values can decode consecutive
  fields from one byte slice.
- Verify public access and representative round trips without changing the
  serialized format or derive-macro behavior.

### Scope

- Specify public exposure and behavior of `EncodeRaw`, `DecodeRaw`, and their
  existing primitive implementations in `src/serialized_struct/mod.rs`.
- Specify the existing `from_le_bytes_only` convenience method and the role
  of `SerializedStruct` as a supporting trait.
- Add opt-in `try_encode` and `try_decode` methods for checked use without
  changing the existing infallible raw methods.
- Verify the public module path made available by the crate root.
- Preserve the signatures and byte representation used by existing consumers.

## Assumptions and dependencies

- The crate root publicly declares the `serialized_struct` module.
- `EncodeRaw` and `DecodeRaw` remain traits in the `serialized_struct` module;
  no additional external dependency is required to use them.
- Raw numeric values use the standard Rust `to_le_bytes` and `from_le_bytes`
  conversions.
- Callers of the raw methods provide slices large enough for the values being
  encoded or decoded; those methods remain infallible and do not return a
  structured short-buffer error. Callers with uncertain buffer lengths may use
  `try_encode` and `try_decode`.
- The derive macros in `serialized_struct_derive` consume these traits, but
  changes to macro behavior are not part of this requirement.
- The crate uses Rust edition 2024. The project-wide MSRV applies.

## Requirements

### Functional requirements

| ID | Requirement |
|---|---|
| RSF-FR-001 | The crate shall expose the `serialized_struct` module publicly so downstream code can address the raw serialization traits through the crate's public API. |
| RSF-FR-002 | The module shall expose public `EncodeRaw` and `DecodeRaw` traits under `serialized_struct::EncodeRaw` and `serialized_struct::DecodeRaw`. |
| RSF-FR-003 | `EncodeRaw::encode` shall write the receiver's little-endian bytes at the beginning of the supplied buffer and return the number of bytes written. |
| RSF-FR-004 | `DecodeRaw::from_le_bytes_slices` shall decode one value beginning at the caller-provided offset and advance the offset by the decoded value's byte width. |
| RSF-FR-005 | `DecodeRaw::from_le_bytes_only` shall decode from offset zero and ignore trailing bytes beyond the decoded value. |
| RSF-FR-006 | Primitive implementations shall cover the currently supported signed and unsigned integers, pointer-sized integers, and `f32`/`f64`: `u8`, `u16`, `u32`, `u64`, `u128`, `usize`, `i8`, `i16`, `i32`, `i64`, `i128`, `isize`, `f32`, and `f64`. |
| RSF-FR-007 | The `SerializedStruct` trait shall remain public, extend both raw traits, and expose `SIZE` for existing fixed-layout serialization consumers. |
| RSF-FR-008 | `SerializedStruct::try_encode` shall verify the destination has at least `SIZE` bytes before calling `encode`, returning `Ok(bytes_written)` on success and `Err` for an undersized destination. |
| RSF-FR-009 | `SerializedStruct::try_decode` shall verify the source has at least `SIZE` bytes before decoding, returning the decoded value on success and `Err` for insufficient input. Trailing bytes shall be ignored, matching `from_le_bytes_only`. |
| RSF-FR-010 | The checked methods shall be opt-in and shall not change the signatures, behavior, or execution path of `encode`, `from_le_bytes_slices`, or `from_le_bytes_only`. |

### Data and serialization

| ID | Requirement |
|---|---|
| RSF-DS-001 | Integer and floating-point representations shall use their standard Rust little-endian byte encodings with no added header or framing bytes. |
| RSF-DS-002 | Sequential decoding shall consume bytes in field order by updating the shared offset after each successful field decode. |
| RSF-DS-003 | Encoded width shall match the primitive type's `size_of::<T>()`; `SerializedStruct::SIZE` for primitive implementations shall match that width. |
| RSF-DS-004 | `usize` and `isize` use the target architecture's width. Cross-architecture persistence of pointer-sized values is not guaranteed by this interface and shall not be implied. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| RSF-RS-001 | `EncodeRaw` shall encode through `&self` into a caller-owned mutable byte slice and return `usize`. |
| RSF-RS-002 | `DecodeRaw` shall construct an owned `Self` from a borrowed byte slice; offset-based decoding shall mutate only the supplied `usize` offset. |
| RSF-RS-003 | The traits shall not impose additional `Copy`, `Clone`, or serialization-library bounds beyond the requirements of each implementation. |
| RSF-RS-004 | Existing public trait and method names and signatures shall remain source-compatible. |
| RSF-RS-005 | Checked methods shall return `anyhow::Result`; they shall require only the existing `SerializedStruct` size metadata and shall not add `Clone` or `Copy` bounds. |

## Public API

The required trait surface is:

```rust
pub trait EncodeRaw {
    fn encode(&self, data: &mut [u8]) -> usize;
}

pub trait DecodeRaw {
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self;

    fn from_le_bytes_only(data: &[u8]) -> Self
    where
        Self: Sized;
}

pub trait SerializedStruct: DecodeRaw + EncodeRaw {
    const SIZE: usize;

    fn try_encode(&self, data: &mut [u8]) -> anyhow::Result<usize>
    where
        Self: Sized;

    fn try_decode(data: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized;
}
```

The public path shall be:

```rust
use rust_serial_file::serialized_struct::{DecodeRaw, EncodeRaw};
```

Checked calls are available for types implementing `SerializedStruct`:

```rust
use rust_serial_file::serialized_struct::SerializedStruct;

let written = value.try_encode(&mut output)?;
let decoded = MyRecord::try_decode(&input)?;
```

- `encode` returns the number of bytes emitted; callers are responsible for
  providing a sufficiently large output slice.
- `from_le_bytes_slices` reads one value at the current offset and advances
  that offset by the value width.
- `from_le_bytes_only` is a convenience operation starting at byte zero.
- `try_encode` and `try_decode` validate the buffer length against `SIZE`
  before delegating to the existing raw operation. They accept buffers longer
  than `SIZE`; trailing input is ignored by `try_decode`.
- Primitive implementations retain the type's standard little-endian
  representation.

## Errors and failure behavior

- The current trait methods do not return `Result`; this requirement preserves
  their infallible signatures.
- A buffer that is too short for an encode or decode operation violates the
  caller precondition. The existing implementations may panic through slice
  indexing or byte-array conversion; callers shall validate buffer lengths
  before calling.
- The offset used with `from_le_bytes_slices` must identify a valid range
  containing the complete encoded value. The current API does not provide
  checked offset arithmetic or a typed decoding error.
- `try_encode` and `try_decode` return `Err` when their respective buffer is
  shorter than `SerializedStruct::SIZE`; they must not partially encode or
  begin decoding in that case.
- The checked methods may use descriptive `anyhow` errors identifying the
  operation, required size, and available size. They shall not include
  serialized contents in errors.
- This requirement does not silently reinterpret malformed or incomplete
  bytes as a valid value.

## Safety and security

- Safe Rust is sufficient; `unsafe` code is not required.
- Byte slices may originate from files or other untrusted sources. Callers must
  use the checked methods when buffer size is not already guaranteed; the raw
  methods retain their existing panic-on-short-buffer precondition.
- Implementations shall not read outside the supplied slice.
- The fixed-width encoding is not self-describing and does not validate
  application-level invariants in decoded values.

## Compatibility

- Existing derive-macro expansions and serialization consumers rely on the
  names and signatures of these traits; this work shall preserve them. Derived
  types implementing `SerializedStruct` shall gain the checked defaults
  without requiring derive macro changes.
- The byte order remains little-endian and primitive byte widths remain
  unchanged.
- Public exposure through the crate root shall not require downstream callers
  to depend on a private implementation module.
- Pointer-sized integer encoding remains architecture-width-dependent.
- A future checked decoding API or portable representation for `usize` and
  `isize` requires a separately reviewed compatibility change.

## Performance and resource use

- Primitive encoding and decoding shall operate directly on the caller's
  buffer without allocating.
- The existing raw methods shall not incur additional checks or error-handling
  overhead because checked methods are separate opt-in calls. Checked calls
  perform a size check and construct an error only on failure.
- No throughput, alignment, or cross-platform persistence guarantee beyond
  the specified primitive representations is imposed.
- The caller owns all input and output buffers; the traits do not retain them.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Confirm public module paths, trait signatures, supported primitive types, and byte/offset semantics against the existing API. | None. | The specified API matches current consumers and its compatibility constraints are explicit. |
| P-002 | Expose the traits and add checked methods with public-API, primitive round-trip, buffer-boundary, and sequential-offset tests. | P-001. | Downstream-style imports compile; supported values encode/decode correctly, checked calls reject undersized buffers, and raw calls retain their behavior. |
| P-003 | Run targeted tests, formatting, Clippy, and coverage; document results and any remaining uncovered code. | P-002. | Relevant checks pass and coverage is reported for the raw serialization module without excluding its production code. |

### Current implementation checklist

- [x] P-001 — `Confirm raw serialization API contract` — Complete
- [x] P-002 — `Expose traits and add behavior tests` — Complete
- [x] P-003 — `Validate raw serialization API` — Complete

**Planning reflection:**

- Confirming the existing signatures and encoding behavior before changing
  exports avoids accidental serialized-format or source-compatibility changes.
- Public import tests and primitive byte tests are independent enough to
  validate API visibility and encoding semantics separately.
- Checked methods provide safer handling for undersized buffers while keeping
  the low-level infallible methods unchanged, so callers select the validation
  cost appropriate to their context.

**Implementation reflection:**

- `SerializedStruct` now supplies default checked methods. Each validates
  `SIZE` before delegating to the existing raw operation, leaving raw method
  signatures and behavior unchanged.
- Public integration tests exercise all supported primitive types, check
  sequential offsets and buffer boundaries, and verify a derived struct
  inherits the checked methods without derive-macro changes.
- The focused coverage run reports complete coverage for
  `src/serialized_struct/mod.rs`.

## Testing and acceptance criteria

### Tests

The following tests have been implemented and executed. Their statuses use the
template's required labels: **✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `RSF-FR-001`, `RSF-FR-002`, `RSF-FR-010`, `RSF-RS-004` | Compile imports of the raw traits and verify existing raw signatures remain available through the public crate path. | The public API imports compile and raw methods remain source-compatible. | `rust_serial_file/tests/serialized_struct_raw.rs` | `raw_traits_are_publicly_accessible` | ✅ Success |
| T-002 | `RSF-FR-003`, `RSF-FR-006`, `RSF-DS-001` | Verify supported primitive values encode to expected little-endian bytes and return the expected width. | Output bytes match little-endian representations and the returned width is correct. | `rust_serial_file/tests/serialized_struct_raw.rs` | `primitive_values_encode_as_little_endian_bytes` | ✅ Success |
| T-003 | `RSF-FR-004`, `RSF-FR-005`, `RSF-FR-006`, `RSF-DS-002` | Decode primitive values sequentially with a shared offset and verify decoding from offset zero. | Decoded values match their originals; offset advances by each encoded width; trailing bytes are ignored by `from_le_bytes_only`. | `rust_serial_file/tests/serialized_struct_raw.rs` | `primitive_values_decode_and_advance_offsets` | ✅ Success |
| T-004 | `RSF-FR-007`, `RSF-DS-003` | Verify primitive `SerializedStruct::SIZE` matches the type width. | `SIZE == size_of::<T>()` for each supported primitive. | `rust_serial_file/tests/serialized_struct_raw.rs` | `primitive_serialized_size_matches_type_width` | ✅ Success |
| T-005 | `RSF-FR-008`, `RSF-FR-010`, `RSF-RS-005` | Verify `try_encode` succeeds with adequate or oversized buffers and returns an error for an undersized buffer without changing the raw API. | Adequate buffers contain the expected bytes; undersized buffers return `Err` without partial output mutation. | `rust_serial_file/tests/serialized_struct_raw.rs` | `checked_encode_validates_buffer_size` | ✅ Success |
| T-006 | `RSF-FR-009`, `RSF-FR-010`, `RSF-RS-005` | Verify `try_decode` for adequate and oversized buffers and reject undersized input. | Valid input decodes correctly, trailing bytes are ignored, and short input returns `Err` rather than panicking. | `rust_serial_file/tests/serialized_struct_raw.rs` | `checked_decode_validates_buffer_size` | ✅ Success |
| T-007 | `RSF-FR-007`, `RSF-FR-008`, `RSF-FR-009` | Verify a derived `SerializedStruct` retains its generated implementation and inherits the checked methods. | Derived struct encodes and decodes successfully through `try_encode` and `try_decode`. | `rust_serial_file/tests/serialized_struct_raw.rs` | `derived_serialized_struct_uses_checked_methods` | ✅ Success |

### Acceptance criteria

1. The raw traits are publicly accessible through the crate's
   `serialized_struct` module.
2. Their names, signatures, and existing primitive byte format remain
   compatible.
3. Encoding widths, little-endian bytes, decode offsets, and primitive
   `SerializedStruct::SIZE` are covered by tests.
4. `try_encode` and `try_decode` report undersized buffers without changing
   the raw methods or their performance path.
5. Existing derive-macro consumers continue to compile.
6. Applicable tests, formatting, lint, build, and coverage checks pass.

## Code coverage

- **Coverage summary:** `cargo llvm-cov --manifest-path
  rust_serial_file/Cargo.toml --test serialized_struct_raw --summary-only`
  reports 100.00% line, region, and function coverage for
  `src/serialized_struct/mod.rs` (34 lines, 42 regions, 5 functions).
- The focused test selector also reports unrelated modules as uncovered; those
  modules are outside this requirement's measurement scope. No production code
  in `src/serialized_struct/mod.rs` was excluded.
- **Validation:** `cargo test --manifest-path rust_serial_file/Cargo.toml`,
  `cargo clippy --manifest-path rust_serial_file/Cargo.toml --all-targets --
  -D warnings`, and `cargo fmt --manifest-path rust_serial_file/Cargo.toml --
  --check` all pass.

## Out of scope

- Changing the existing `encode`, `from_le_bytes_slices`, or
  `from_le_bytes_only` signatures to return `Result`.
- Providing checked offset-based decoding in this iteration; `try_decode`
  validates a complete value from the start of the supplied slice.
- Redesigning behavior for malformed input beyond short-buffer validation.
- Changing derive macro parsing or generated implementations.
- Adding support for unions, enums, strings, collections, or additional
  floating-point formats.
- Defining a portable, architecture-independent representation for `usize`
  and `isize`.
- Changing the on-disk format used by existing file consumers.
