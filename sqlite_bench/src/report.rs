//! Builds the Markdown summary from criterion's results.
//!
//! Benchmark ids follow `<operation>/<store>[_<profile>]/<record count>`,
//! where `<store>` is `struct_file`, `struct_blob_file` or `sqlite` and
//! `<profile>` is `fast` or `durable` (write benchmarks only). Criterion
//! writes each result to
//! `<criterion dir>/<operation>/<function>/<count>/new/{benchmark,estimates}.json`.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Display order of the operations; unknown ones come after, by name.
pub const OPERATIONS: [&str; 14] = [
    "append_one",
    "append_batch",
    "scan",
    "random_read",
    "search_hit",
    "search_miss",
    "range",
    "update",
    "insert_remove",
    "open",
    "named_append",
    "named_scan",
    "named_update",
    "file_size",
];

/// One criterion result.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Benchmark group: the operation.
    pub operation: String,
    /// Store name (`struct_file`, `struct_blob_file`, `sqlite`).
    pub store: String,
    /// Profile name, or `None` for read-only operations.
    pub profile: Option<String>,
    /// Number of records.
    pub records: u64,
    /// Mean time of one iteration, in nanoseconds.
    pub mean_ns: f64,
}

/// One line of the summary: both storages for one operation, size and
/// profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The operation.
    pub operation: String,
    /// Number of records.
    pub records: u64,
    /// Profile name, or `None` for read-only operations.
    pub profile: Option<String>,
    /// `StructFile` (or `StructBlobFile`) mean time, ns.
    pub serial_ns: Option<f64>,
    /// SQLite mean time, ns.
    pub sqlite_ns: Option<f64>,
}

impl Line {
    /// SQLite time divided by `StructFile` time: above 1, `StructFile` is
    /// faster.
    pub fn ratio(&self) -> Option<f64> {
        Some(self.sqlite_ns? / self.serial_ns?)
    }
}

/// Splits a criterion function id into store and profile.
pub fn split_function(function: &str) -> (String, Option<String>) {
    for profile in ["fast", "durable"] {
        if let Some(store) = function.strip_suffix(&format!("_{profile}")) {
            return (store.to_owned(), Some(profile.to_owned()));
        }
    }
    (function.to_owned(), None)
}

/// Reads every result under `criterion_dir`.
///
/// # Errors
///
/// Fails if the directory cannot be read or a result file is malformed.
pub fn collect(criterion_dir: &Path) -> Result<Vec<Estimate>> {
    let mut found = Vec::new();
    find_results(criterion_dir, &mut found)?;
    let mut estimates = Vec::new();
    for directory in found {
        let benchmark = read_json(&directory.join("benchmark.json"))?;
        let estimates_json = read_json(&directory.join("estimates.json"))?;
        let operation = json_str(&benchmark, "group_id")?;
        let function = json_str(&benchmark, "function_id")?;
        let records: u64 = json_str(&benchmark, "value_str")?
            .parse()
            .with_context(|| format!("non-numeric record count in {}", directory.display()))?;
        let mean_ns = estimates_json["mean"]["point_estimate"]
            .as_f64()
            .with_context(|| format!("no mean estimate in {}", directory.display()))?;
        let (store, profile) = split_function(&function);
        estimates.push(Estimate {
            operation,
            store,
            profile,
            records,
            mean_ns,
        });
    }
    Ok(estimates)
}

/// Collects every `new` directory holding a `benchmark.json`.
fn find_results(directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?;
    for entry in entries {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "new") {
            if path.join("benchmark.json").exists() {
                found.push(path);
            }
        } else {
            find_results(&path, found)?;
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn json_str(value: &Value, key: &str) -> Result<String> {
    match &value[key] {
        Value::String(text) => Ok(text.clone()),
        _ => bail!("missing string field {key:?}"),
    }
}

/// Pairs the storages of each (operation, size, profile), in display order.
pub fn lines(estimates: &[Estimate]) -> Vec<Line> {
    let order = |operation: &str| {
        OPERATIONS
            .iter()
            .position(|known| *known == operation)
            .unwrap_or(OPERATIONS.len())
    };
    // Read-only operations (no profile) first, then fast, then durable.
    let profile_rank = |profile: &Option<String>| match profile.as_deref() {
        None => 0,
        Some("fast") => 1,
        Some("durable") => 2,
        Some(_) => 3,
    };
    let mut grouped: BTreeMap<(usize, String, u64, usize, Option<String>), Line> = BTreeMap::new();
    for estimate in estimates {
        let key = (
            order(&estimate.operation),
            estimate.operation.clone(),
            estimate.records,
            profile_rank(&estimate.profile),
            estimate.profile.clone(),
        );
        let line = grouped.entry(key).or_insert_with(|| Line {
            operation: estimate.operation.clone(),
            records: estimate.records,
            profile: estimate.profile.clone(),
            serial_ns: None,
            sqlite_ns: None,
        });
        if estimate.store == "sqlite" {
            line.sqlite_ns = Some(estimate.mean_ns);
        } else {
            line.serial_ns = Some(estimate.mean_ns);
        }
    }
    grouped.into_values().collect()
}

/// Formats nanoseconds with a readable unit.
pub fn format_ns(ns: f64) -> String {
    if ns < 1e3 {
        format!("{ns:.0} ns")
    } else if ns < 1e6 {
        format!("{:.2} µs", ns / 1e3)
    } else if ns < 1e9 {
        format!("{:.2} ms", ns / 1e6)
    } else {
        format!("{:.2} s", ns / 1e9)
    }
}

/// Formats a byte count with a readable unit.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// On-disk size of both storages for one record count.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeLine {
    /// What was stored (`samples` or `named`).
    pub data: String,
    /// Number of records.
    pub records: u64,
    /// `StructFile` (or `StructBlobFile`) bytes.
    pub serial_bytes: u64,
    /// SQLite bytes (database and WAL).
    pub sqlite_bytes: u64,
}

/// Renders the summary: environment, timing table, size table.
pub fn render(environment: &[(String, String)], lines: &[Line], sizes: &[SizeLine]) -> String {
    let mut out = String::new();
    out.push_str("# StructFile vs SQLite benchmark summary\n\n");
    out.push_str("Generated by `cargo run --release --bin sqlite_bench_report` from the results of\n`cargo bench`. Times are criterion means of one iteration (warm OS cache).\n");
    out.push_str("Ratio = SQLite time / StructFile time: above 1, StructFile is faster.\n\n");
    out.push_str("## Environment\n\n| Item | Value |\n|---|---|\n");
    for (item, value) in environment {
        let _ = writeln!(out, "| {item} | {value} |");
    }
    out.push_str("\n## Timings\n\n| Operation | Records | Profile | StructFile | SQLite | Ratio |\n|---|---:|---|---:|---:|---:|\n");
    for line in lines {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            line.operation,
            line.records,
            line.profile.as_deref().unwrap_or("—"),
            line.serial_ns.map_or("—".to_owned(), format_ns),
            line.sqlite_ns.map_or("—".to_owned(), format_ns),
            line.ratio()
                .map_or("—".to_owned(), |ratio| format!("{ratio:.2}")),
        );
    }
    if !sizes.is_empty() {
        out.push_str("\n## File sizes\n\n| Data | Records | StructFile | SQLite | StructFile bytes/record | SQLite bytes/record |\n|---|---:|---:|---:|---:|---:|\n");
        for size in sizes {
            let per = |bytes: u64| format!("{:.1}", bytes as f64 / size.records.max(1) as f64);
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} |",
                size.data,
                size.records,
                format_bytes(size.serial_bytes),
                format_bytes(size.sqlite_bytes),
                per(size.serial_bytes),
                per(size.sqlite_bytes),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_ns, split_function};

    #[test]
    fn formats_units_and_ids() {
        assert_eq!(format_ns(512.0), "512 ns");
        assert_eq!(format_ns(1_500.0), "1.50 µs");
        assert_eq!(format_ns(2_500_000.0), "2.50 ms");
        assert_eq!(format_ns(3_000_000_000.0), "3.00 s");
        assert_eq!(format_bytes(10), "10 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(
            split_function("struct_file_durable"),
            ("struct_file".to_owned(), Some("durable".to_owned()))
        );
        assert_eq!(split_function("sqlite"), ("sqlite".to_owned(), None));
    }
}
