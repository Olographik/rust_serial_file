//! Writes `results/summary.md` from criterion's results (run `cargo bench`
//! first) and measures the on-disk size of both storages.
//!
//! Usage: `sqlite_bench_report [criterion dir] [output file]`, by default
//! `target/criterion` and `results/summary.md` in this crate. The record
//! counts measured for the file sizes follow `SQLITE_BENCH_SIZES` /
//! `SQLITE_BENCH_LARGE` like the benchmarks.

use std::{env, fs, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use sqlite_bench::{
    Profile, Store,
    named_store::{BlobNamedStore, NamedStore, SqliteNamedStore},
    record::{generate, generate_names},
    report::{SizeLine, collect, lines, render},
    serial_store::SerialStore,
    sqlite_store::SqliteStore,
};

const SEED: u64 = 0x5EED;
const MAX_NAMED_RECORDS: usize = 100_000;

fn sizes() -> Vec<usize> {
    if let Ok(list) = env::var("SQLITE_BENCH_SIZES") {
        return list
            .split(',')
            .filter_map(|size| size.trim().parse().ok())
            .collect();
    }
    let mut sizes = vec![1_000, 100_000, 1_000_000];
    if env::var("SQLITE_BENCH_LARGE").is_ok_and(|value| value == "1") {
        sizes.push(10_000_000);
    }
    sizes
}

/// Builds both storages (fast profile, batches of 1 000, as the append
/// benchmark) and measures them.
fn file_sizes() -> Result<Vec<SizeLine>> {
    let mut lines = Vec::new();
    for size in sizes() {
        let directory = tempfile::tempdir()?;
        let samples = generate(size, SEED);
        let mut serial = SerialStore::create(directory.path(), Profile::Fast)?;
        serial.append_batches(&samples, 1_000)?;
        let mut sqlite = SqliteStore::create(directory.path(), Profile::Fast)?;
        sqlite.append_batches(&samples, 1_000)?;
        lines.push(SizeLine {
            data: "samples".to_owned(),
            records: size as u64,
            serial_bytes: serial.disk_size()?,
            sqlite_bytes: sqlite.disk_size()?,
        });

        if size <= MAX_NAMED_RECORDS {
            let names = generate_names(size, SEED);
            let mut blob = BlobNamedStore::create(directory.path(), Profile::Fast)?;
            blob.append_batches(&names, 1_000)?;
            let mut sqlite = SqliteNamedStore::create(directory.path(), Profile::Fast)?;
            sqlite.append_batches(&names, 1_000)?;
            lines.push(SizeLine {
                data: "named".to_owned(),
                records: size as u64,
                serial_bytes: blob.disk_size()?,
                sqlite_bytes: sqlite.disk_size()?,
            });
        }
    }
    Ok(lines)
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn cpu() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split(':').nth(1))
                .map(|name| name.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn main() -> Result<()> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut args = env::args().skip(1);
    let criterion_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("target/criterion"));
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("results/summary.md"));

    let estimates = collect(&criterion_dir).with_context(|| {
        format!(
            "no criterion results in {}; run `cargo bench` first",
            criterion_dir.display()
        )
    })?;
    let environment = vec![
        ("Rust".to_owned(), command_output("rustc", &["--version"])),
        ("SQLite".to_owned(), format!("{} (bundled by rusqlite)", rusqlite::version())),
        ("OS".to_owned(), format!("{} {}", env::consts::OS, env::consts::ARCH)),
        ("Kernel".to_owned(), command_output("uname", &["-r"])),
        ("CPU".to_owned(), format!("{} ({} threads)", cpu(), std::thread::available_parallelism().map_or(0, usize::from))),
        ("Profiles".to_owned(), "fast: SQLite WAL + synchronous=OFF, StructFile without sync; durable: SQLite WAL + synchronous=FULL, commit per batch, StructFile sync_data per batch".to_owned()),
    ];
    let summary = render(&environment, &lines(&estimates), &file_sizes()?);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, summary).with_context(|| format!("failed to write {}", output.display()))?;
    println!("wrote {} ({} results)", output.display(), estimates.len());
    Ok(())
}
