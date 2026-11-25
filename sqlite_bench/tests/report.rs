use std::path::Path;

use sqlite_bench::report::{SizeLine, collect, lines, render};

#[test]
fn builds_summary_table() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/criterion");
    let estimates = collect(&fixtures).unwrap();
    // Only the `new` results are read; `base` and `report` are ignored.
    assert_eq!(estimates.len(), 8);

    let environment = vec![("Rust".to_owned(), "test".to_owned())];
    let sizes = vec![SizeLine {
        data: "samples".to_owned(),
        records: 1000,
        serial_bytes: 24_000,
        sqlite_bytes: 49_000,
    }];
    let summary = render(&environment, &lines(&estimates), &sizes);
    let expected_table = "\
| Operation | Records | Profile | StructFile | SQLite | Ratio |
|---|---:|---|---:|---:|---:|
| append_one | 1000 | fast | 1.75 ms | 400.00 µs | 0.23 |
| append_one | 1000 | durable | 3.00 ms | 1.50 ms | 0.50 |
| scan | 1000 | — | 2.00 µs | 8.00 µs | 4.00 |
| scan | 100000 | — | 250.00 µs | — | — |
| custom_op | 10 | — | — | 5 ns | — |
";
    assert!(summary.contains(expected_table), "{summary}");
    assert!(summary.contains("| Rust | test |"));
    assert!(
        summary.contains("| samples | 1000 | 23.4 KiB | 47.9 KiB | 24.0 | 49.0 |"),
        "{summary}"
    );

    assert!(collect(&fixtures.join("missing")).is_err());
}

#[test]
fn report_binary_writes_the_summary() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/criterion");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("out/summary.md");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_sqlite_bench_report"))
        .arg(&fixtures)
        .arg(&output)
        .env("SQLITE_BENCH_SIZES", "200")
        .status()
        .unwrap();
    assert!(status.success());
    let summary = std::fs::read_to_string(&output).unwrap();
    assert!(summary.contains("| append_one | 1000 | fast | 1.75 ms | 400.00 µs | 0.23 |"));
    assert!(summary.contains("| SQLite | 3."), "{summary}");
    // File sizes are measured for the requested count, with and without names.
    assert!(summary.contains("| samples | 200 | 4.7 KiB |"), "{summary}");
    assert!(summary.contains("| named | 200 |"), "{summary}");

    // Without results, the binary fails with a clear message.
    let failed = std::process::Command::new(env!("CARGO_BIN_EXE_sqlite_bench_report"))
        .arg(directory.path().join("missing"))
        .arg(&output)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("run `cargo bench` first"));
}

#[test]
fn rejects_malformed_results() {
    let directory = tempfile::tempdir().unwrap();
    let new = directory.path().join("op/sqlite/10/new");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(
        new.join("benchmark.json"),
        r#"{"group_id":"op","value_str":"10"}"#,
    )
    .unwrap();
    std::fs::write(
        new.join("estimates.json"),
        r#"{"mean":{"point_estimate":1.0}}"#,
    )
    .unwrap();
    let error = format!("{:#}", collect(directory.path()).unwrap_err());
    assert!(error.contains("function_id"), "{error}");

    std::fs::write(
        new.join("benchmark.json"),
        r#"{"group_id":"op","function_id":"sqlite_weird","value_str":"ten"}"#,
    )
    .unwrap();
    assert!(collect(directory.path()).is_err());
    std::fs::write(new.join("benchmark.json"), "not json").unwrap();
    assert!(collect(directory.path()).is_err());
}
