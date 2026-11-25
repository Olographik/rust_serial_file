# Random Generator Injection Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.1
- **Short description:** Let callers of the `utils` performance `Tester` inject their own random generator, and make `rand` an optional dependency so `utils` (and crates depending on it) can build for WebAssembly.

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

`utils` has two modules: `search` (binary and linear search, used by
`rust_serial_file`) and `performance` (timing helpers and `Tester`, which runs
benchmark actions in a random order). Only `performance` uses `rand`:

- `Tester::execute_one_loop` and `Tester::execute_with_rand` take a
  `T: rand::Rng`.
- `Tester::execute` calls `rand::rng()` (OS entropy through `getrandom`).
- `Tester::execute_seeded` builds a `rand::rngs::StdRng`.

Because `rand` is an unconditional dependency, every crate depending on
`utils` pulls `rand` and `getrandom`, which do not build for
`wasm32-unknown-unknown` without extra configuration. Today this blocks the
browser client through `rust_serial_file → utils → rand`.

This document specifies a small random-index trait owned by `utils`, so the
caller injects the generator (a `rand` generator, a browser generator such as
one backed by `crypto.getRandomValues`, a fixed sequence for tests, ...), and
cargo features that make `rand` and the `performance` module optional.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|

## Goals and scope

### Goals

- `Tester` accepts any generator implementing a `utils` trait, not only
  `rand::Rng`.
- `utils` builds with no `rand`/`getrandom` in its dependency tree when its
  default features are disabled.
- `rust_serial_file` depends on `utils` without `rand`.
- Existing calls (`execute`, `execute_seeded`, `execute_with_rand(&mut rng)`
  with a `rand` generator) keep compiling with default features.

### Scope

- `utils/src/performance/mod.rs` (`Tester`), `utils/src/search/compare_binary_search.rs`
  (uses `Tester`), `utils/src/main.rs`, `utils/Cargo.toml`.
- `rust_serial_file/Cargo.toml` and `rust_python_file/Cargo.toml` dependency
  declarations on `utils`.
- Unchanged: the `search` module API and behavior, `PerfomanceMeasure`,
  `AveragePerformance`, `Action`, `ActionAgent`.

## Assumptions and dependencies

- `rand` 0.10 stays the optional default generator.
- Features of `utils`:
  - `rand` (default): `rand` dependency, adapter for `rand::Rng`,
    `Tester::execute`.
  - `performance` (default): the `performance` module and
    `search::compare_binary_search`.
- The `utils` binary (`src/main.rs`) declares
  `required-features = ["performance", "rand"]`.
- Edition 2024.

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `RNG-UTL-FR-001` | `utils` shall define a public trait `IndexRng` with `fn next_index(&mut self, upper: usize) -> usize` returning a value in `0..upper` (`upper > 0`). | 0.1 |
| `RNG-UTL-FR-002` | `Tester::execute_with_rand` and `Tester::execute_one_loop` shall accept any `R: IndexRng + ?Sized` instead of `T: rand::Rng`. | 0.1 |
| `RNG-UTL-FR-003` | With the `rand` feature, every `R: rand::Rng` shall implement `IndexRng` (blanket impl), so `execute_with_rand(&mut rand::rng())` keeps compiling. | 0.1 |
| `RNG-UTL-FR-004` | `utils` shall provide `FnIndexRng<F>` (built with `IndexRng::from_fn` or `FnIndexRng::new`), implementing `IndexRng` for any `F: FnMut(usize) -> usize`, so a caller can inject a closure (for example one calling a JavaScript random source). | 0.1 |
| `RNG-UTL-FR-005` | `utils` shall provide `SplitMix64`, a dependency-free deterministic generator implementing `IndexRng`, built from a `u64` seed. | 0.1 |
| `RNG-UTL-FR-006` | `Tester::execute_seeded(seed)` shall be available without the `rand` feature and use `SplitMix64`. | 0.1 |
| `RNG-UTL-FR-007` | `Tester::execute()` (OS entropy) shall exist only with the `rand` feature. | 0.1 |
| `RNG-UTL-FR-008` | The `performance` module and `search::compare_binary_search` shall exist only with the `performance` feature; `search` (binary, linear, search types) shall always be available. | 0.1 |
| `RNG-UTL-FR-009` | `rust_serial_file` shall depend on `utils` with `default-features = false`. `rust_python_file` keeps the defaults. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `RNG-UTL-DS-001` | `SplitMix64` shall produce the same sequence for the same seed on every platform (pure 64-bit arithmetic, no `usize` in the state). | 0.1 |
| `RNG-UTL-DS-002` | `next_index` shall map the 64-bit output to `0..upper` without modulo bias above 2⁻³² (for example Lemire's multiply-shift on a `u64`). | 0.1 |
| `RNG-UTL-DS-003` | Out-of-range values from an injected generator are a caller bug: `Tester` shall reduce them with `% upper` before indexing rather than panicking on the index. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `RNG-UTL-RS-001` | `IndexRng` shall be object safe, so `&mut dyn IndexRng` can be passed. | 0.1 |
| `RNG-UTL-RS-002` | `cargo build -p utils --no-default-features --target wasm32-unknown-unknown` shall succeed, and `cargo tree -p utils --no-default-features -e normal` shall not list `rand` or `getrandom`. | 0.1 |
| `RNG-UTL-RS-003` | No `unsafe` code. | 0.1 |

## Public API

```rust
pub mod random {
    pub trait IndexRng {
        /// Returns a value in `0..upper`. `upper` is never 0 when called by `Tester`.
        fn next_index(&mut self, upper: usize) -> usize;
    }

    pub struct SplitMix64 { /* u64 state */ }
    impl SplitMix64 { pub fn new(seed: u64) -> Self; pub fn next_u64(&mut self) -> u64; }
    impl IndexRng for SplitMix64 { /* ... */ }

    pub struct FnIndexRng<F>(F);
    impl<F: FnMut(usize) -> usize> FnIndexRng<F> { pub fn new(f: F) -> Self; }
    impl<F: FnMut(usize) -> usize> IndexRng for FnIndexRng<F> { /* ... */ }

    #[cfg(feature = "rand")]
    impl<R: rand::Rng + ?Sized> IndexRng for R { /* random_range(0..upper) */ }
}

#[cfg(feature = "performance")]
impl Tester {
    pub fn execute_one_loop<R: IndexRng + ?Sized>(actions: &mut Vec<&mut ActionAgent>, rng: &mut R);
    pub fn execute_with_rand<R: IndexRng + ?Sized>(&mut self, rng: &mut R);
    pub fn execute_seeded(&mut self, seed: u64);
    #[cfg(feature = "rand")]
    pub fn execute(&mut self);
}
```

Note: a blanket `impl<R: rand::Rng> IndexRng for R` and a blanket impl for
closures would overlap for the compiler, which is why closures go through the
`FnIndexRng` wrapper.

## Errors and failure behavior

- `next_index` is infallible. An injected generator that cannot produce a
  value must handle it itself (for example fall back to a seeded generator).
- An out-of-range result is reduced, not reported (`RNG-UTL-DS-003`).
- `upper == 0` is never passed by `Tester`; other callers get an unspecified
  value and implementations may panic. `SplitMix64` shall panic with a clear
  message on `upper == 0`.

## Safety and security

- None: no `unsafe` code is allowed.
- The generators are for benchmark ordering only and are not
  cryptographically secure; this shall be stated in the doc comments.

## Compatibility

- Source compatible with default features for `execute`,
  `execute_with_rand(&mut <rand generator>)` and `execute_one_loop`.
- Behavior change: `execute_seeded(seed)` produces a different action order
  than today for the same seed (`StdRng` → `SplitMix64`). It only affects the
  order of benchmark actions, not measured results.
- Crates that use `utils::performance` with `default-features = false` must
  enable the `performance` feature.

## Performance and resource use

- `SplitMix64` is a few arithmetic operations per call; negligible next to
  the measured actions.
- Non-goal: changing measurement precision or the timing source.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Trait and generators:** add `utils::random` with `IndexRng`, `SplitMix64`, `FnIndexRng`, and the `rand` blanket impl behind the feature. | None | Unit tests T-001 to T-004 pass. |
| P-002 | **Tester:** switch `Tester` to `IndexRng`, move `execute_seeded` to `SplitMix64`, gate `execute` on `rand`. | P-001 | Existing performance test and `compare_binary_search` still pass; T-005, T-006 pass. |
| P-003 | **Features and dependents:** add `rand` and `performance` features, `required-features` on the binary, set `default-features = false` in `rust_serial_file`. | P-002 | T-007 and T-008 pass; `rust_serial_file` and `rust_python_file` tests pass. |
| P-004 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-003 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-005 | **Documentation:** update this requirements document and the code comments of the touched files. | P-004 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [ ] P-001 — `IndexRng trait and generators` — Not started
- [ ] P-002 — `Tester uses injected generator` — Not started
- [ ] P-003 — `Cargo features and dependents` — Not started
- [ ] P-004 — `Coverage of touched files` — Not started
- [ ] P-005 — `Documentation update` — Not started

**Planning reflection (complete before implementation):**

- The trait is needed before `Tester` can change, and features can only be
  gated once nothing outside the gate uses `rand`, hence this order.
- Each phase leaves the crate building with default features.
- Risk: the existing `test_simple_performance` test asserts timing windows of
  1 ms and is flaky on loaded machines; it is not changed by this work.
- Decisions to confirm: the `execute_seeded` sequence change; gating the
  whole `performance` module (it also uses `SystemTime::now`, which panics on
  `wasm32-unknown-unknown`).

**Implementation reflection (update as work progresses):**

- None yet.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `RNG-UTL-FR-005`, `RNG-UTL-DS-001` | `SplitMix64::new(seed).next_u64()` against published reference values. | Values equal the reference sequence. |
| T-002 | `RNG-UTL-FR-001`, `RNG-UTL-DS-002` | Draw 100 000 `next_index(7)` from `SplitMix64`. | All values in `0..7`; every value appears. |
| T-003 | `RNG-UTL-FR-004`, `RNG-UTL-RS-001` | `FnIndexRng` with a closure returning a fixed sequence, used through `&mut dyn IndexRng`. | Returns the closure's values. |
| T-004 | `RNG-UTL-FR-003` | `next_index` on a seeded `StdRng` (feature `rand`). | Values in range; compiles via blanket impl. |
| T-005 | `RNG-UTL-FR-002`, `RNG-UTL-DS-003` | `Tester` with a `FnIndexRng` always returning `0`, then one returning out-of-range values. | Every action runs `loop_count` times in the injected order; no panic. |
| T-006 | `RNG-UTL-FR-006` | Two `execute_seeded(42)` runs record their action order. | Same order both times. |
| T-007 | `RNG-UTL-FR-007`, `RNG-UTL-FR-008`, `RNG-UTL-RS-002` | `cargo build --no-default-features` (native and `wasm32-unknown-unknown`), `cargo tree` check, `cargo build --features performance --no-default-features`. | Builds succeed; no `rand`/`getrandom` without the feature. |
| T-008 | `RNG-UTL-FR-009` | `cargo tree -p rust_serial_file -e normal`. | `rand` and `getrandom` absent. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `utils/src/random.rs` | `splitmix_reference_values` |
| T-002 | `utils/src/random.rs` | `splitmix_index_in_range` |
| T-003 | `utils/src/random.rs` | `fn_index_rng_returns_closure_values` |
| T-004 | `utils/src/random.rs` | `rand_generator_is_index_rng` |
| T-005 | `utils/src/performance/mod.rs` | `tester_uses_injected_generator` |
| T-006 | `utils/src/performance/mod.rs` | `seeded_execution_is_reproducible` |
| T-007 | Shell check (documented command) | `no-default-features builds` |
| T-008 | Shell check (documented command) | `rust_serial_file dependency tree` |

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

#### Notes

- None.

#### Warning notes

- None.

### Acceptance criteria

1. A caller can run `Tester` with a generator of its choice.
2. `utils --no-default-features` and `rust_serial_file` have no `rand` in
   their dependency tree.
3. Existing default-feature users compile unchanged.
4. `cargo fmt`, `cargo clippy`, build and tests pass.

## Code coverage

- **Coverage summary:** Not run yet.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --summary-only` (planned)
- **Date of run:** —
- **Git hash:** —

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `utils/src/performance/mod.rs` | — | — | — | — |
| `utils/src/random.rs` | — | — | — | — |

### Coverage notes

- Not run yet.

## Out of scope

- Injecting the clock: `PerfomanceMeasure` uses `SystemTime::now()`, which
  panics on `wasm32-unknown-unknown`. Gating the module is enough for the
  current need; a `Clock` trait can be added later if benchmarks must run in
  the browser. Decision: repository owner.
- Cryptographically secure randomness.
- Moving `performance` to its own crate or to dev-dependencies only.
