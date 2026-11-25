# SQLite Comparison Benchmark Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.2
- **Short description:** A separate benchmark crate that measures the read and write performance of `StructFile` against an equivalent SQLite database on the same time-series workload, with comparable durability settings.

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

`rust_serial_file` stores ordered, fixed-width records in a flat file. SQLite
is the usual alternative for the same job. This document specifies a new
workspace crate, `sqlite_bench/`, that runs the same operations on both
storages with the same data and reports the timings and file sizes side by
side, so the strengths and costs of `StructFile` are measured rather than
assumed.

It is a benchmark tool, not a library: nothing in the other crates depends on
it.

## Revisions

| Revision | Date | Author | Changes |
|---|---|---|---|
| 0.2 | 2026-11-25 | Olographik | Planning reflection: `criterion` confirmed as the benchmark harness; document approved. No requirement changed. |

## Goals and scope

### Goals

- Compare write, read, search, update, insert and delete performance of
  `StructFile` and SQLite on an ordered time-series record.
- Make the comparison fair: same records, same order, same machine, and
  matching durability levels.
- Produce results that can be re-run and compared over time (stored reports
  and a summary table).

### Scope

- New crate `sqlite_bench/` with benchmark code under `benches/` and shared
  setup code in `src/`.
- Storages: `StructFile` (and `BufferedWriter` for batch writes) on one side,
  SQLite through `rusqlite` on the other.
- Later phase: dynamic strings (blob file vs SQLite `TEXT`), once
  `dynamic_storage.md` is implemented.
- Unchanged: every other crate. The benchmark only uses public APIs.

## Assumptions and dependencies

- Dependencies (versions chosen at implementation time, latest stable):
  - `rust_serial_file`, `serialized_struct` (once extracted) and
    `serialized_struct_derive` by path.
  - `rusqlite` with the `bundled` feature, so the SQLite version is fixed by
    the crate and not by the machine (needs a C compiler).
  - `criterion` as the benchmark harness (warm-up, repeated samples,
    outlier detection, HTML reports, comparison with the previous run).
  - `tempfile` for benchmark files.
- Benchmarks run on a local disk; the files' directory can be chosen with an
  environment variable (`SQLITE_BENCH_DIR`) to test another disk.
- Results are "warm cache" (OS page cache not dropped between runs), because
  dropping caches needs root. This is stated in the report.
- Edition 2024. Native targets only (no WebAssembly).

## Requirements

### Functional requirements

| ID | Requirement | Revision |
|---|---|---|
| `SQC-SQB-FR-001` | The crate shall define one benchmark record `Sample { time: u64, sensor: u32, flags: u32, value: f64 }` (24 bytes) deriving `SerializedStruct`, stored in SQLite as table `sample(time INTEGER PRIMARY KEY, sensor INTEGER NOT NULL, flags INTEGER NOT NULL, value REAL NOT NULL) WITHOUT ROWID`. | 0.1 |
| `SQC-SQB-FR-002` | A deterministic generator shall produce the records from a seed (strictly increasing `time`), so both storages receive identical data in identical order on every run. | 0.1 |
| `SQC-SQB-FR-003` | Each benchmark shall run for data sizes of 1 000, 100 000 and 1 000 000 records. A larger size (10 000 000) shall be available behind an environment variable (`SQLITE_BENCH_LARGE=1`). | 0.1 |
| `SQC-SQB-FR-004` | **Append one by one:** `StructFile::serial_write` per record vs one `INSERT` per record with a prepared statement inside one transaction. | 0.1 |
| `SQC-SQB-FR-005` | **Append in batch:** `StructFile::write_multiple` and `BufferedWriter` vs one transaction of prepared `INSERT`s, for batches of 1 000 records. | 0.1 |
| `SQC-SQB-FR-006` | **Sequential scan:** read every record in order (`serial_iter`) vs `SELECT … ORDER BY time`, decoding into `Sample` on both sides. | 0.1 |
| `SQC-SQB-FR-007` | **Random read by position:** 10 000 reads at seeded random positions (`read_value_at`) vs `SELECT … WHERE time = ?` with the time at that position (the position itself has no meaning in SQLite; documented). | 0.1 |
| `SQC-SQB-FR-008` | **Search by key:** 10 000 seeded lookups of existing and missing `time` values (`serial_find` binary search) vs `SELECT … WHERE time = ?`. | 0.1 |
| `SQC-SQB-FR-009` | **Range query:** 1 000 seeded ranges of 100 records (find the first index, then iterate) vs `SELECT … WHERE time BETWEEN ? AND ? ORDER BY time`. | 0.1 |
| `SQC-SQB-FR-010` | **Update in place:** 10 000 seeded updates of `value` (`write_value_at`) vs `UPDATE … SET value = ? WHERE time = ?` in one transaction. | 0.1 |
| `SQC-SQB-FR-011` | **Insert in the middle and remove:** 100 inserts at seeded positions keeping `time` order (`serial_insert`) and 100 removals (`serial_remove`) vs `INSERT` / `DELETE` in one transaction. Expected to favor SQLite; reported as is. | 0.1 |
| `SQC-SQB-FR-012` | **Open:** time to open an existing file and read the first record, for both storages. | 0.1 |
| `SQC-SQB-FR-013` | **File size:** after the append benchmark, report the on-disk size of each storage (SQLite main file plus WAL/journal if present) and bytes per record. | 0.1 |
| `SQC-SQB-FR-014` | **Durability profiles:** every write benchmark runs under two profiles. `fast`: SQLite `journal_mode=WAL`, `synchronous=OFF`; `StructFile` without sync. `durable`: SQLite `journal_mode=WAL`, `synchronous=FULL`, commit per batch; `StructFile` with `sync_data` after each batch (needs `file_sync.md`). | 0.1 |
| `SQC-SQB-FR-015` | A binary `sqlite_bench_report` shall read the criterion results and write a Markdown summary table (operation, size, profile, StructFile time, SQLite time, ratio) to `sqlite_bench/results/summary.md`. | 0.1 |
| `SQC-SQB-FR-016` | Later phase: **dynamic strings**: records with a 16 to 256 byte string, stored with the blob file (`dynamic_storage.md`) vs a SQLite `TEXT` column, for append, scan and update. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `SQC-SQB-DS-001` | Before measuring, each benchmark shall check that both storages hold the same records (count and a checksum over all fields); a mismatch fails the run. | 0.1 |
| `SQC-SQB-DS-002` | `time` is unique and increasing, so it is both the `StructFile` order and the SQLite primary key; no extra SQLite index is created. | 0.1 |
| `SQC-SQB-DS-003` | Every read benchmark shall decode values into `Sample` and pass them to `criterion::black_box`, so neither side is measured without decoding or optimized away. | 0.1 |
| `SQC-SQB-DS-004` | Benchmark files are created fresh per benchmark group in a temporary directory and deleted afterwards. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `SQC-SQB-RS-001` | The crate is `publish = false` and is not a dependency of any other crate. | 0.1 |
| `SQC-SQB-RS-002` | Benchmarks use only public APIs of `rust_serial_file`; no feature or test-only hook is added to it for the benchmark. | 0.1 |
| `SQC-SQB-RS-003` | No `unsafe` code in the crate. | 0.1 |
| `SQC-SQB-RS-004` | Benchmarks are built in release mode (`cargo bench`); the report records the `rustc` version, SQLite version, OS and CPU. | 0.1 |

## Public API

No library API. Entry points:

```text
cargo bench -p sqlite_bench                        # all benchmarks
cargo bench -p sqlite_bench -- append              # one group
SQLITE_BENCH_LARGE=1 cargo bench -p sqlite_bench   # include 10M records
cargo run -p sqlite_bench --release --bin sqlite_bench_report
```

Internal modules (`src/`): `record` (Sample, generator, checksum),
`serial_store` and `sqlite_store` (setup and operations behind the same small
interface so each benchmark is written once per operation), `profile`
(durability profiles).

## Errors and failure behavior

- Any I/O or SQLite error during setup or a measured operation aborts the
  benchmark with the error; no partial result is reported as valid.
- A data mismatch between the storages (`SQC-SQB-DS-001`) aborts the run.
- A profile that cannot run yet (for example `durable` before `sync_data`
  exists) is skipped with a printed message, not silently measured without
  sync.

## Safety and security

- None: no `unsafe` code is allowed in this crate. `rusqlite` with `bundled`
  compiles SQLite's C code, which is an accepted third-party dependency of
  this benchmark only.
- Only generated data is used; no untrusted input.

## Compatibility

- No effect on the other crates or their APIs.
- Benchmark names stay stable so criterion can compare runs over time; a
  renamed benchmark loses its history.

## Performance and resource use

- The 1 000 000-record runs need about 24 MB per `StructFile` and a similar
  amount per SQLite file, plus copies per benchmark group; the 10 M option
  needs about ten times that.
- A full run is expected to take tens of minutes; groups can be run alone.
- Non-goal: tuning SQLite beyond the documented profiles (page size, mmap,
  cache size). The profile settings are listed in the report so readers know
  what was compared.

## Phases

### Phase planning and reflection

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | **Crate and data:** create `sqlite_bench/`, record type, generator, checksum, both store setups, data equality check. | `serialized_struct_crate.md` P-002 (import paths). | T-001 to T-003 pass. |
| P-002 | **Write benchmarks:** append one by one and in batch, `fast` profile; file size report. | P-001 | `cargo bench -- append` runs for all sizes; T-004 passes. |
| P-003 | **Read benchmarks:** scan, random read, search, range, open. | P-001 | The groups run for all sizes. |
| P-004 | **Modify benchmarks:** update, insert in the middle, remove. | P-001 | The groups run for all sizes. |
| P-005 | **Durable profile:** add `durable` to the write and modify groups. | P-002, P-004, `file_sync.md` P-002. | Both profiles reported; T-005 passes. |
| P-006 | **Report:** `sqlite_bench_report` binary and first `results/summary.md`, with environment details. | P-002 to P-005 | T-006 passes; summary committed. |
| P-007 | **Dynamic strings:** blob file vs `TEXT`. | P-006, `dynamic_storage.md` P-003. | The group runs and appears in the summary. |
| P-008 | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | P-001 to P-007 | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-009 | **Documentation:** update this requirements document (with the first results) and the code comments of the touched files. | P-008 | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

- [ ] P-001 — `Crate, record and data setup` — Not started
- [ ] P-002 — `Write benchmarks` — Not started
- [ ] P-003 — `Read benchmarks` — Not started
- [ ] P-004 — `Modify benchmarks` — Not started
- [ ] P-005 — `Durable profile` — Not started
- [ ] P-006 — `Summary report` — Not started
- [ ] P-007 — `Dynamic strings` — Not started
- [ ] P-008 — `Coverage of touched files` — Not started
- [ ] P-009 — `Documentation update` — Not started

**Planning reflection (complete before implementation):**

- P-001 to P-004 depend only on today's API (plus the crate split for import
  paths) and give useful numbers early. P-005 waits for `sync_data`, P-007
  for the blob file, so they come last.
- Risk: unfair comparisons. Mitigated by identical data checks, decoding on
  both sides, and explicit durability profiles. Without them, `StructFile`
  (no sync) would be compared to SQLite's default (sync on every commit),
  which mostly measures the disk.
- Risk: noisy machines (CI, laptops). Results are indicative for the machine
  that produced them; the report records the environment.
- Confirmed by Olographik (2026-11-25): `criterion` is the harness, not
  `utils::performance::Tester` (criterion gives statistics and run-to-run
  comparison).

**Implementation reflection (update as work progresses):**

- None yet.

## Testing and acceptance criteria

### Tests

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `SQC-SQB-FR-002` | Generate twice with the same seed; with another seed. | Same seed gives identical records; `time` strictly increasing; other seed differs. |
| T-002 | `SQC-SQB-FR-001`, `SQC-SQB-DS-001`, `SQC-SQB-DS-002` | Load 10 000 records into both stores and compare count and checksum. | Equal. |
| T-003 | `SQC-SQB-DS-001` | Corrupt one SQLite row, run the equality check. | Check reports the mismatch. |
| T-004 | `SQC-SQB-FR-004` to `SQC-SQB-FR-013` | Run every operation of both stores once on 1 000 records outside criterion (smoke test) and check results (found records, range lengths, final counts). | Same observable results on both stores. |
| T-005 | `SQC-SQB-FR-014` | Open the SQLite connection with each profile and query the pragmas. | Pragmas match the profile. |
| T-006 | `SQC-SQB-FR-015` | Run the report binary on a fixed sample of criterion output. | Produces the expected Markdown table. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `sqlite_bench/src/record.rs` | `generator_is_deterministic` |
| T-002 | `sqlite_bench/tests/stores.rs` | `stores_hold_same_data` |
| T-003 | `sqlite_bench/tests/stores.rs` | `detects_mismatch` |
| T-004 | `sqlite_bench/tests/stores.rs` | `operations_agree` |
| T-005 | `sqlite_bench/src/profile.rs` | `profiles_set_pragmas` |
| T-006 | `sqlite_bench/tests/report.rs` | `builds_summary_table` |

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

1. Every operation in `SQC-SQB-FR-004` to `SQC-SQB-FR-013` is measured on
   both storages for every size, under both durability profiles where they
   apply.
2. Both storages are verified to hold the same data before measuring.
3. A Markdown summary with the environment details is produced and
   committed.
4. `cargo fmt`, `cargo clippy`, build, tests and `cargo bench` pass.

## Code coverage

- **Coverage summary:** Not run yet. Coverage applies to the setup code in
  `src/` and the report binary; the benchmark bodies are exercised by T-004.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --summary-only` (planned)
- **Date of run:** —
- **Git hash:** —

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `sqlite_bench/src/profile.rs` | — | — | — | — |
| `sqlite_bench/src/record.rs` | — | — | — | — |
| `sqlite_bench/src/serial_store.rs` | — | — | — | — |
| `sqlite_bench/src/sqlite_store.rs` | — | — | — | — |

### Coverage notes

- Not run yet.

## Out of scope

- Cold-cache measurements (needs root to drop the OS page cache).
- Concurrent readers and writers (`FileAtlas` vs SQLite with several
  connections); could be a later group. Decision: Olographik.
- Other databases (DuckDB, RocksDB, Parquet files).
- Tuning SQLite beyond the two documented profiles.
- Running the benchmarks in CI.
