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
