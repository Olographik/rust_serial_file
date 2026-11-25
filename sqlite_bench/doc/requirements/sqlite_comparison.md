# SQLite Comparison Benchmark Requirements

- **Date:** 2026-11-25
- **Author:** `Olographik`
- **Revision:** 0.4
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
| 0.3 | 2026-11-25 | Olographik | Implemented (P-001 to P-009) and first results recorded in `sqlite_bench/results/summary.md`. Modified `SQC-SQB-FR-005` (batch append as one block write), `SQC-SQB-FR-008` (separate hit and miss benchmarks), `SQC-SQB-FR-009` (range as `time >= ? LIMIT ?`), `SQC-SQB-FR-011` (insert and remove in the same iteration), `SQC-SQB-FR-013` (sizes measured by the report binary), `SQC-SQB-FR-016` (sizes up to 100 000, both profiles), `SQC-SQB-DS-004` (fixture reuse). Added `SQC-SQB-FR-017` (`SQLITE_BENCH_SIZES`, `SQLITE_BENCH_DIR`). Tests T-007 and T-008 added; T-005 location corrected. Checklist, results, coverage and reflection updated. |
| 0.4 | 2026-11-25 | Olographik | Modified `SQC-SQB-FR-005`: batch appends use `StructFile::write_multiple(len, batch)` now that it takes a start index (`struct_file.md` revision 0.6). The `append_batch` group was re-run and the summary regenerated. Scope, reflection and notes updated. |

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
- Storages: `StructFile` (`write_multiple` at the end for batch appends) on
  one side,
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
| `SQC-SQB-FR-005` | **Append in batch:** batches of 1 000 records; `StructFile`: `write_multiple(serial_len(), batch)` (one block write per batch); SQLite: one transaction of prepared `INSERT`s per batch. | 0.4 |
| `SQC-SQB-FR-006` | **Sequential scan:** read every record in order (`serial_iter`) vs `SELECT … ORDER BY time`, decoding into `Sample` on both sides. | 0.1 |
| `SQC-SQB-FR-007` | **Random read by position:** 10 000 reads at seeded random positions (`read_value_at`) vs `SELECT … WHERE time = ?` with the time at that position (the position itself has no meaning in SQLite; documented). | 0.1 |
| `SQC-SQB-FR-008` | **Search by key:** 10 000 seeded lookups of existing times (`search_hit`) and, separately, of missing times (`search_miss`): `StructFile::serial_find_by` (binary search, then reading the row) vs `SELECT … WHERE time = ?`. | 0.3 |
| `SQC-SQB-FR-009` | **Range query:** 1 000 seeded ranges of 100 records starting at an existing time: `StructFile` finds the first index and reads the rows with one buffered row iterator; SQLite runs `SELECT … WHERE time >= ? ORDER BY time LIMIT ?` (same rows as the `StructFile` side, which a `BETWEEN` on times could not guarantee). | 0.3 |
| `SQC-SQB-FR-010` | **Update in place:** 10 000 seeded updates of `value` (`write_value_at`) vs `UPDATE … SET value = ? WHERE time = ?` in one transaction. | 0.1 |
| `SQC-SQB-FR-011` | **Insert in the middle and remove:** one iteration inserts up to 100 records after seeded positions keeping `time` order (`serial_insert`, descending positions) and removes them again (`serial_remove`), vs `INSERT` and `DELETE` transactions. The data is unchanged after each iteration, so no file copy is needed. Expected to favor SQLite; reported as is. | 0.3 |
| `SQC-SQB-FR-012` | **Open:** time to open an existing file and read the first record, for both storages. | 0.1 |
| `SQC-SQB-FR-013` | **File size:** the report binary builds each storage the way the batch append benchmark does (fast profile, batches of 1 000) for every benchmark size and reports its on-disk size (SQLite: main file plus WAL; the `-shm` shared-memory index is excluded) and bytes per record. | 0.3 |
| `SQC-SQB-FR-014` | **Durability profiles:** every write benchmark runs under two profiles. `fast`: SQLite `journal_mode=WAL`, `synchronous=OFF`; `StructFile` without sync. `durable`: SQLite `journal_mode=WAL`, `synchronous=FULL`, commit per batch; `StructFile` with `sync_data` after each batch (needs `file_sync.md`). | 0.1 |
| `SQC-SQB-FR-015` | A binary `sqlite_bench_report` shall read the criterion results and write a Markdown summary table (operation, size, profile, StructFile time, SQLite time, ratio) to `sqlite_bench/results/summary.md`. | 0.1 |
| `SQC-SQB-FR-016` | **Dynamic strings:** records with a 16 to 256 byte string, `StructBlobFile` (rows with a `BlobRef` + blob file) vs a SQLite `TEXT` column (`named(time INTEGER PRIMARY KEY, name TEXT NOT NULL) WITHOUT ROWID`), for sizes up to 100 000: append in batches of 1 000 (both profiles), full scan, and 1 000 updates with longer names on a fresh copy of the files (both profiles; in the blob file every updated value moves). | 0.3 |
| `SQC-SQB-FR-017` | `SQLITE_BENCH_SIZES=1000,100000` shall restrict the record counts (benchmarks and report), and `SQLITE_BENCH_DIR=<dir>` shall place the benchmark files on another disk. | 0.3 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `SQC-SQB-DS-001` | Before measuring, each benchmark shall check that both storages hold the same records (count and a checksum over all fields); a mismatch fails the run. | 0.1 |
| `SQC-SQB-DS-002` | `time` is unique and increasing, so it is both the `StructFile` order and the SQLite primary key; no extra SQLite index is created. | 0.1 |
| `SQC-SQB-DS-003` | Every read benchmark shall decode values into `Sample` and pass them to `criterion::black_box`, so neither side is measured without decoding or optimized away. | 0.1 |
| `SQC-SQB-DS-004` | Benchmark files live in a temporary directory (or under `SQLITE_BENCH_DIR`) and are deleted afterwards. Append benchmarks start from a new, empty storage for every iteration; read and modification benchmarks share one loaded fixture per size, checked equal on both sides first (`SQC-SQB-DS-001`), and leave it equal. | 0.3 |

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

- [x] P-001 — `Crate, record and data setup` — Done
- [x] P-002 — `Write benchmarks` — Done
- [x] P-003 — `Read benchmarks` — Done
- [x] P-004 — `Modify benchmarks` — Done
- [x] P-005 — `Durable profile` — Done
- [x] P-006 — `Summary report` — Done
- [x] P-007 — `Dynamic strings` — Done
- [x] P-008 — `Coverage of touched files` — Done
- [x] P-009 — `Documentation update` — Done

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

- Implemented on 2026-11-25 with `rusqlite` 0.43 (SQLite 3.54.1 bundled),
  `criterion` 0.8.2 and `serde_json` for the report. No phase was added or
  reordered; the durable profile (P-005) and the dynamic strings (P-007)
  could be built directly since `file_sync.md` and `dynamic_storage.md` were
  already implemented.
- **Batch append (`SQC-SQB-FR-005` modified twice).** In revision 0.3,
  `StructFile::write_multiple` always wrote from row 0, so the benchmark
  appended through `FileMetadata::write_rows` and rebuilt the `StructFile`
  for each batch. The owner considered row 0 a bug; `struct_file.md`
  revision 0.6 gave `write_multiple` a start index, and since revision 0.4
  of this document the benchmark calls `write_multiple(len, batch)`. The
  re-run `append_batch` group is equal or slightly faster (for example
  16.85 ms instead of 20.07 ms for 1 000 000 records, fast profile), as
  nothing is rebuilt per batch.
- **Range (`SQC-SQB-FR-009` modified).** `BETWEEN` on times would not
  return exactly the 100 rows the `StructFile` side reads, so SQLite uses
  `time >= ? ORDER BY time LIMIT ?`.
- **Insert/remove (`SQC-SQB-FR-011` modified).** Inserting and removing the
  same rows in one iteration leaves the files unchanged, so the fixture is
  reused without copying 1 000 000-record files for every iteration.
- **Dynamic strings (`SQC-SQB-FR-016` modified).** Updates need a fresh
  copy of the files per iteration (moved values change the blob file), so
  these benchmarks stop at 100 000 records.
- **Sampling.** The first full run used criterion's default linear sampling
  (55 iterations for 10 samples), which makes each 1 000 000-record append
  take minutes. It was stopped and every group switched to flat sampling
  (10 iterations of the same size).
- **One test assertion was written with made-up values** (an exact expected
  sample) and removed before it ran; the test checks range lengths instead.

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
| T-007 | `SQC-SQB-FR-016` | Load 2 000 names into both named stores in two calls, compare checksums, update every 7th name with a longer one, compare again; update a missing record. | Checksums equal before and after; the missing record is an error. |
| T-008 | `SQC-SQB-FR-015` | Unit formatting of times, sizes and benchmark ids. | Expected strings. |

#### Test location

| Test ID | File | Test name |
|---|---|---|
| T-001 | `sqlite_bench/src/record.rs` | `generator_is_deterministic` |
| T-002 | `sqlite_bench/tests/stores.rs` | `stores_hold_same_data` |
| T-003 | `sqlite_bench/tests/stores.rs` | `detects_mismatch` |
| T-004 | `sqlite_bench/tests/stores.rs` | `operations_agree` |
| T-005 | `sqlite_bench/src/sqlite_store.rs` (unit test) | `profiles_set_pragmas` |
| T-006 | `sqlite_bench/tests/report.rs` | `builds_summary_table` |
| T-007 | `sqlite_bench/tests/stores.rs` | `named_stores_agree` |
| T-008 | `sqlite_bench/src/report.rs` (unit test) | `formats_units_and_ids` |

#### Test results

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | 2026-11-25 | `2209624` | ✅ Success |
| T-002 | 2026-11-25 | `2209624` | ✅ Success |
| T-003 | 2026-11-25 | `2209624` | ✅ Success |
| T-004 | 2026-11-25 | `2209624` | ✅ Success |
| T-005 | 2026-11-25 | `2209624` | ✅ Success |
| T-006 | 2026-11-25 | `2209624` | ✅ Success |
| T-007 | 2026-11-25 | `2209624` | ✅ Success |
| T-008 | 2026-11-25 | `2209624` | ✅ Success |

#### Notes

- Tests: `cargo test` in `sqlite_bench/` (11 tests, all pass) on `2209624`;
  `cargo fmt --check` and `cargo clippy --all-targets` are clean. Besides
  T-001 to T-008, `stores_can_be_recreated_and_reopened`,
  `report_binary_writes_the_summary` and `rejects_malformed_results` cover
  re-creating stores, the report binary end to end and malformed criterion
  files.
- Benchmarks: `cargo bench --bench compare` on `79b0df1` (the benchmark code
  is unchanged since), sizes 1 000 / 100 000 / 1 000 000, 104 results in
  14 minutes, on a 4-thread Xeon VM with an ext4 virtual disk. Results are
  in `sqlite_bench/results/summary.md`, generated with
  `cargo run --release --bin sqlite_bench_report`. Criterion warned that six
  slow groups needed more than the 5 s target; it still took 10 samples.
- Durability on a cloud VM depends on how the host honors `fsync`; the
  durable numbers are indicative for this machine only.
- What the first results show (ratio = SQLite / StructFile):
  - `StructFile` is clearly faster for **batch appends** (34-40× fast,
    2.5-4× durable), **full scans** (about 4.3×), **reads by position**
    (3.5-5.3×) and **opening** a file (51-60×), and its files are smaller
    (24 bytes per record vs 31-65 for SQLite with its WAL).
  - SQLite is clearly faster for **appends one record at a time** (about
    4×: each `serial_write` makes several system calls, SQLite buffers the
    transaction), **search by key** (about 4×: the `StructFile` binary
    search reads every probed row from the file, SQLite searches its
    in-memory page cache), **ranges** (about 2.4×, dominated by that search),
    and by far for **inserting and removing in the middle** (up to ~900× at
    1 000 000 records, since `StructFile` shifts every following row).
  - **Updates** are close; `StructFile` wins only for the fast profile at
    1 000 000 records.
  - **Dynamic strings**: SQLite is faster for appends (2-4×) and scans
    (1.4-1.8×); updates with longer values are even at 100 000 records
    (fast) and favor SQLite otherwise. Each blob value costs a separate
    positioned read and write.
- These point to possible improvements of `rust_serial_file`, not part of
  this work: buffering in `serial_write`, a cached or buffered binary
  search, and reading several blob values per call. See Out of scope. (The
  batch append is now `write_multiple` with a start index.)
- Revision 0.4: only the `append_batch` group was re-run, on `8531747`
  (`cargo bench --bench compare -- append_batch`); every other result in
  `results/summary.md` is from the full run on `79b0df1`. Batch-append
  ratios are now 33.6-39.5 (fast) and 2.5-3.8 (durable).

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

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** measured with the crate's tests (not the benchmark
  bodies, which only run under `cargo bench`; T-004 and T-007 run the same
  operations). `record.rs` is fully covered; every other file has at least
  86% of its lines covered.

### Current coverage results

- **Tool and command:** `cargo llvm-cov --summary-only` (cargo-llvm-cov 0.9.2) in `sqlite_bench/`
- **Date of run:** 2026-11-25
- **Git hash:** `2209624`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `sqlite_bench/src/bin/sqlite_bench_report.rs` | 68.42% (13/19) | 86.79% (92/106) | 81.73% (170/208) | N/A |
| `sqlite_bench/src/lib.rs` | 100.00% (4/4) | 100.00% (24/24) | 88.37% (38/43) | N/A |
| `sqlite_bench/src/named_store.rs` | 100.00% (17/17) | 99.21% (125/126) | 88.50% (254/287) | N/A |
| `sqlite_bench/src/record.rs` | 100.00% (18/18) | 100.00% (101/101) | 100.00% (161/161) | N/A |
| `sqlite_bench/src/report.rs` | 91.30% (21/23) | 97.60% (163/167) | 94.95% (282/297) | N/A |
| `sqlite_bench/src/serial_store.rs` | 100.00% (20/20) | 100.00% (120/120) | 90.87% (219/241) | N/A |
| `sqlite_bench/src/sqlite_store.rs` | 92.31% (24/26) | 95.97% (143/149) | 86.69% (280/323) | N/A |

### Coverage notes

Branch coverage is `N/A` for every file (needs a nightly toolchain). The
missed regions not listed below are `?` error paths of file and SQLite
operations that only fail on I/O errors or a corrupt database.

- **`sqlite_bench/src/bin/sqlite_bench_report.rs`:** the default record
  counts (used when `SQLITE_BENCH_SIZES` is not set) are not run by the test,
  which would otherwise build 1 000 000-record stores; they ran when the
  committed summary was generated. The other missed functions are error
  closures (`rustc` or `uname` missing, no `/proc/cpuinfo`, output directory
  not writable).
- **`sqlite_bench/src/lib.rs`:** all lines covered; the 5 missed regions
  are the `?` error paths of `scan` and `len` in `check_same_data` and
  `is_empty`.
- **`sqlite_bench/src/named_store.rs`:** line 182, the `?` of the `CREATE
  TABLE` statement, which only fails on an existing table in a fresh file.
- **`sqlite_bench/src/report.rs`:** an unknown profile name in the sort
  order (lines 164) and the summary without a file size table (line 267),
  which the binary always produces.
- **`sqlite_bench/src/serial_store.rs`:** all lines covered.
- **`sqlite_bench/src/sqlite_store.rs`:** `?` paths of SQLite statements
  (lines 70, 100, 160) and a pragma returning neither an integer nor text
  (line 213).

## Out of scope

- Cold-cache measurements (needs root to drop the OS page cache).
- Concurrent readers and writers (`FileAtlas` vs SQLite with several
  connections); could be a later group. Decision: Olographik.
- Other databases (DuckDB, RocksDB, Parquet files).
- Tuning SQLite beyond the two documented profiles.
- Running the benchmarks in CI.
- Improving `rust_serial_file` from these results (buffered
  `serial_write`, cached binary search, batched blob reads). Each would be its own requirement; decision: Olographik.
