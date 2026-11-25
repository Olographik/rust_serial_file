//! StructFile vs SQLite benchmarks. See `doc/requirements/sqlite_comparison.md`.
//!
//! Environment variables:
//! - `SQLITE_BENCH_LARGE=1`: also run 10 000 000 records.
//! - `SQLITE_BENCH_SIZES=1000,100000`: run only these record counts.
//! - `SQLITE_BENCH_DIR=<dir>`: put the benchmark files in `<dir>` (default:
//!   the system temporary directory).

use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Duration,
};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use sqlite_bench::{
    Profile, Store, check_same_data,
    named_store::{BlobNamedStore, NamedStore, SqliteNamedStore},
    record::{Sample, generate, generate_names, missing_time_after},
    serial_store::SerialStore,
    sqlite_store::SqliteStore,
};
use tempfile::TempDir;
use utils::random::{IndexRng, SplitMix64};

const SEED: u64 = 0x5EED;
const BATCH: usize = 1_000;
const LOOKUPS: usize = 10_000;
const RANGES: usize = 1_000;
const RANGE_LEN: usize = 100;
const UPDATES: usize = 10_000;
const INSERTS: usize = 100;
const NAME_UPDATES: usize = 1_000;
/// Dynamic-string benchmarks stop at this size: above it, copying the files
/// for every `named_update` iteration costs more disk than the benchmark
/// is worth.
const MAX_NAMED_RECORDS: usize = 100_000;

fn sizes() -> Vec<usize> {
    if let Ok(list) = env::var("SQLITE_BENCH_SIZES") {
        return list
            .split(',')
            .map(|size| {
                size.trim()
                    .parse()
                    .expect("SQLITE_BENCH_SIZES: comma-separated counts")
            })
            .collect();
    }
    let mut sizes = vec![1_000, 100_000, 1_000_000];
    if env::var("SQLITE_BENCH_LARGE").is_ok_and(|value| value == "1") {
        sizes.push(10_000_000);
    }
    sizes
}

fn temp_dir() -> TempDir {
    match env::var_os("SQLITE_BENCH_DIR") {
        Some(dir) => {
            fs::create_dir_all(&dir).expect("SQLITE_BENCH_DIR cannot be created");
            tempfile::tempdir_in(dir).expect("cannot create a directory in SQLITE_BENCH_DIR")
        }
        None => tempfile::tempdir().expect("cannot create a temporary directory"),
    }
}

/// Both storages loaded with the same samples, checked equal.
struct Fixture {
    directory: TempDir,
    samples: Vec<Sample>,
}

impl Fixture {
    fn new(size: usize) -> Self {
        let directory = temp_dir();
        let samples = generate(size, SEED);
        let mut serial = SerialStore::create(directory.path(), Profile::Fast).unwrap();
        serial.append_batches(&samples, 10_000).unwrap();
        let mut sqlite = SqliteStore::create(directory.path(), Profile::Fast).unwrap();
        sqlite.append_batches(&samples, 10_000).unwrap();
        check_same_data(&serial, &sqlite).expect("the storages must hold the same data");
        Self { directory, samples }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }
}

/// `count` seeded positions in `0..len`.
fn positions(count: usize, len: usize, seed: u64) -> Vec<usize> {
    let mut rng = SplitMix64::new(seed);
    (0..count).map(|_| rng.next_index(len)).collect()
}

fn configure<'a>(
    criterion: &'a mut Criterion,
    name: &str,
) -> criterion::BenchmarkGroup<'a, criterion::measurement::WallTime> {
    let mut group = criterion.benchmark_group(name);
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group
}

fn bench_appends<S: Store>(criterion: &mut Criterion) {
    for (operation, one_by_one) in [("append_one", true), ("append_batch", false)] {
        let mut group = configure(criterion, operation);
        for size in sizes() {
            let samples = generate(size, SEED);
            for profile in Profile::ALL {
                group.bench_with_input(
                    BenchmarkId::new(format!("{}_{}", S::NAME, profile.name()), size),
                    &samples,
                    |bencher, samples| {
                        bencher.iter_batched(
                            || {
                                let directory = temp_dir();
                                let store = S::create(directory.path(), profile).unwrap();
                                (directory, store)
                            },
                            |(directory, mut store)| {
                                if one_by_one {
                                    store.append_one(samples).unwrap();
                                } else {
                                    store.append_batches(samples, BATCH).unwrap();
                                }
                                (directory, store)
                            },
                            BatchSize::PerIteration,
                        );
                    },
                );
            }
        }
        group.finish();
    }
}

fn bench_reads<S: Store>(criterion: &mut Criterion, fixtures: &[Fixture]) {
    let mut scan = configure(criterion, "scan");
    for fixture in fixtures {
        let store = S::open(fixture.path(), Profile::Fast).unwrap();
        scan.bench_function(
            BenchmarkId::new(S::NAME, fixture.samples.len()),
            |bencher| {
                bencher.iter(|| black_box(store.scan().unwrap()));
            },
        );
    }
    scan.finish();

    let mut random = configure(criterion, "random_read");
    for fixture in fixtures {
        let store = S::open(fixture.path(), Profile::Fast).unwrap();
        let picks: Vec<(u64, u64)> = positions(LOOKUPS, fixture.samples.len(), SEED + 1)
            .into_iter()
            .map(|index| (index as u64, fixture.samples[index].time))
            .collect();
        random.bench_function(
            BenchmarkId::new(S::NAME, fixture.samples.len()),
            |bencher| {
                bencher.iter(|| {
                    for (index, time) in &picks {
                        black_box(store.get(*index, *time).unwrap());
                    }
                });
            },
        );
    }
    random.finish();

    for (operation, hit) in [("search_hit", true), ("search_miss", false)] {
        let mut group = configure(criterion, operation);
        for fixture in fixtures {
            let store = S::open(fixture.path(), Profile::Fast).unwrap();
            let times: Vec<u64> = positions(LOOKUPS, fixture.samples.len(), SEED + 2)
                .into_iter()
                .map(|index| {
                    let sample = &fixture.samples[index];
                    if hit {
                        sample.time
                    } else {
                        missing_time_after(sample)
                    }
                })
                .collect();
            group.bench_function(
                BenchmarkId::new(S::NAME, fixture.samples.len()),
                |bencher| {
                    bencher.iter(|| {
                        for time in &times {
                            let found = store.find(*time).unwrap();
                            assert_eq!(found.is_some(), hit);
                            black_box(found);
                        }
                    });
                },
            );
        }
        group.finish();
    }

    let mut range = configure(criterion, "range");
    for fixture in fixtures {
        let store = S::open(fixture.path(), Profile::Fast).unwrap();
        let len = fixture.samples.len();
        let starts: Vec<u64> = positions(RANGES, len.saturating_sub(RANGE_LEN).max(1), SEED + 3)
            .into_iter()
            .map(|index| fixture.samples[index].time)
            .collect();
        range.bench_function(BenchmarkId::new(S::NAME, len), |bencher| {
            let mut out = Vec::with_capacity(RANGE_LEN);
            bencher.iter(|| {
                for start in &starts {
                    out.clear();
                    store.range(*start, RANGE_LEN, &mut out).unwrap();
                    black_box(&out);
                }
            });
        });
    }
    range.finish();

    let mut open = configure(criterion, "open");
    for fixture in fixtures {
        let first = fixture.samples[0].time;
        open.bench_function(
            BenchmarkId::new(S::NAME, fixture.samples.len()),
            |bencher| {
                bencher.iter(|| {
                    let store = S::open(fixture.path(), Profile::Fast).unwrap();
                    black_box(store.get(0, first).unwrap());
                });
            },
        );
    }
    open.finish();
}

fn bench_modifications<S: Store>(criterion: &mut Criterion, fixtures: &[Fixture]) {
    let mut update = configure(criterion, "update");
    for fixture in fixtures {
        let updates: Vec<(u64, Sample)> = positions(UPDATES, fixture.samples.len(), SEED + 4)
            .into_iter()
            .map(|index| {
                let mut sample = fixture.samples[index];
                sample.value = 100.0 - sample.value;
                (index as u64, sample)
            })
            .collect();
        for profile in Profile::ALL {
            let mut store = S::open(fixture.path(), profile).unwrap();
            update.bench_function(
                BenchmarkId::new(
                    format!("{}_{}", S::NAME, profile.name()),
                    fixture.samples.len(),
                ),
                |bencher| bencher.iter(|| store.update(&updates).unwrap()),
            );
        }
    }
    update.finish();

    // Inserting rows after random positions, then removing them again,
    // leaves the data unchanged, so every iteration starts from the same
    // state without copying files.
    let mut insert_remove = configure(criterion, "insert_remove");
    for fixture in fixtures {
        let mut picks = positions(INSERTS, fixture.samples.len(), SEED + 5);
        picks.sort_unstable();
        picks.dedup();
        picks.reverse();
        let inserts: Vec<(u64, Sample)> = picks
            .iter()
            .map(|&index| {
                let mut sample = fixture.samples[index];
                sample.time = missing_time_after(&fixture.samples[index]);
                (index as u64 + 1, sample)
            })
            .collect();
        // After all inserts (descending), the row inserted after original
        // row `i` sits at `i + 1 + (number of inserts before it)`.
        let count = inserts.len() as u64;
        let removals: Vec<(u64, u64)> = inserts
            .iter()
            .enumerate()
            .map(|(rank, (index, sample))| (index + (count - 1 - rank as u64), sample.time))
            .collect();
        for profile in Profile::ALL {
            let mut store = S::open(fixture.path(), profile).unwrap();
            insert_remove.bench_function(
                BenchmarkId::new(
                    format!("{}_{}", S::NAME, profile.name()),
                    fixture.samples.len(),
                ),
                |bencher| {
                    bencher.iter(|| {
                        store.insert(&inserts).unwrap();
                        store.remove(&removals).unwrap();
                    })
                },
            );
            assert_eq!(store.len().unwrap(), fixture.samples.len() as u64);
        }
    }
    insert_remove.finish();
}

/// Copies every file of `from` into a new directory.
fn copy_dir(from: &Path) -> TempDir {
    let to = temp_dir();
    for entry in fs::read_dir(from).unwrap() {
        let path: PathBuf = entry.unwrap().path();
        fs::copy(&path, to.path().join(path.file_name().unwrap())).unwrap();
    }
    to
}

fn bench_named<S: NamedStore>(criterion: &mut Criterion) {
    let named_sizes: Vec<usize> = sizes()
        .into_iter()
        .filter(|size| *size <= MAX_NAMED_RECORDS)
        .collect();

    let mut append = configure(criterion, "named_append");
    for &size in &named_sizes {
        let names = generate_names(size, SEED);
        for profile in Profile::ALL {
            append.bench_with_input(
                BenchmarkId::new(format!("{}_{}", S::NAME, profile.name()), size),
                &names,
                |bencher, names| {
                    bencher.iter_batched(
                        || {
                            let directory = temp_dir();
                            let store = S::create(directory.path(), profile).unwrap();
                            (directory, store)
                        },
                        |(directory, mut store)| {
                            store.append_batches(names, BATCH).unwrap();
                            (directory, store)
                        },
                        BatchSize::PerIteration,
                    );
                },
            );
        }
    }
    append.finish();

    // One loaded store per size, reused by the scan and copied by updates.
    let bases: Vec<(usize, Vec<String>, TempDir)> = named_sizes
        .iter()
        .map(|&size| {
            let names = generate_names(size, SEED);
            let base = temp_dir();
            let mut store = S::create(base.path(), Profile::Fast).unwrap();
            store.append_batches(&names, 10_000).unwrap();
            (size, names, base)
        })
        .collect();

    let mut scan = configure(criterion, "named_scan");
    for (size, _, base) in &bases {
        let store = S::open(base.path(), Profile::Fast).unwrap();
        scan.bench_function(BenchmarkId::new(S::NAME, *size), |bencher| {
            bencher.iter(|| black_box(store.scan().unwrap()));
        });
    }
    scan.finish();

    let mut update = configure(criterion, "named_update");
    for (size, names, base) in &bases {
        // Longer names: in the blob file every updated value moves.
        let updates: Vec<(usize, String)> = positions(NAME_UPDATES.min(*size), *size, SEED + 6)
            .into_iter()
            .map(|index| {
                (
                    index,
                    format!("{}-renamed-with-a-longer-name", names[index]),
                )
            })
            .collect();
        for profile in Profile::ALL {
            update.bench_function(
                BenchmarkId::new(format!("{}_{}", S::NAME, profile.name()), *size),
                |bencher| {
                    bencher.iter_batched(
                        || {
                            let copy = copy_dir(base.path());
                            let store = S::open(copy.path(), profile).unwrap();
                            (copy, store)
                        },
                        |(copy, mut store)| {
                            store.update(&updates).unwrap();
                            (copy, store)
                        },
                        BatchSize::PerIteration,
                    );
                },
            );
        }
    }
    update.finish();
}

fn all(criterion: &mut Criterion) {
    bench_appends::<SerialStore>(criterion);
    bench_appends::<SqliteStore>(criterion);

    let fixtures: Vec<Fixture> = sizes().into_iter().map(Fixture::new).collect();
    bench_reads::<SerialStore>(criterion, &fixtures);
    bench_reads::<SqliteStore>(criterion, &fixtures);
    bench_modifications::<SerialStore>(criterion, &fixtures);
    bench_modifications::<SqliteStore>(criterion, &fixtures);
    drop(fixtures);

    bench_named::<BlobNamedStore>(criterion);
    bench_named::<SqliteNamedStore>(criterion);
}

criterion_group!(benches, all);
criterion_main!(benches);
