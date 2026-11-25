//! Benchmark support code comparing `StructFile` with SQLite.
//!
//! Both storages implement [`Store`], so each benchmark is written once.
//! The benchmarks themselves are in `benches/compare.rs`; the summary report
//! is built by the `sqlite_bench_report` binary.

pub mod named_store;
pub mod record;
pub mod report;
pub mod serial_store;
pub mod sqlite_store;

use std::path::Path;

use anyhow::Result;

use record::{Checksum, Sample};

/// How much durability a write benchmark asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Nothing is forced to storage. SQLite: `journal_mode=WAL`,
    /// `synchronous=OFF`; `StructFile`: no sync.
    Fast,
    /// Every batch is durable. SQLite: `journal_mode=WAL`,
    /// `synchronous=FULL`, one commit per batch; `StructFile`: `sync_data`
    /// after each batch.
    Durable,
}

impl Profile {
    /// Both profiles, fast first.
    pub const ALL: [Profile; 2] = [Profile::Fast, Profile::Durable];

    /// Lower-case name, used in benchmark ids.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Fast => "fast",
            Profile::Durable => "durable",
        }
    }

    /// Value of SQLite's `synchronous` pragma for this profile.
    pub fn sqlite_synchronous(self) -> &'static str {
        match self {
            Profile::Fast => "OFF",
            Profile::Durable => "FULL",
        }
    }
}

/// One storage of [`Sample`]s ordered by `time`.
///
/// Operations take the positions a `StructFile` needs (`index`) and the keys
/// SQLite needs (`time`); each storage uses what it needs. Every write
/// operation is complete and, under [`Profile::Durable`], durable when it
/// returns.
pub trait Store: Sized {
    /// Name used in benchmark ids (`struct_file`, `sqlite`).
    const NAME: &'static str;

    /// Creates an empty storage in `directory`.
    fn create(directory: &Path, profile: Profile) -> Result<Self>;

    /// Opens the storage previously created in `directory`.
    fn open(directory: &Path, profile: Profile) -> Result<Self>;

    /// Appends samples one by one (one write or `INSERT` per sample, a
    /// single transaction).
    fn append_one(&mut self, samples: &[Sample]) -> Result<()>;

    /// Appends samples in batches of `batch` (one block write or one
    /// transaction per batch).
    fn append_batches(&mut self, samples: &[Sample], batch: usize) -> Result<()>;

    /// Number of samples.
    fn len(&self) -> Result<u64>;

    /// Whether the storage is empty.
    fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Reads every sample in `time` order and returns their checksum.
    fn scan(&self) -> Result<Checksum>;

    /// Reads the sample at `index`, whose time is `time`.
    fn get(&self, index: u64, time: u64) -> Result<Sample>;

    /// Finds the sample with `time`, if any.
    fn find(&self, time: u64) -> Result<Option<Sample>>;

    /// Appends to `out` up to `count` samples starting at the existing time
    /// `start_time`, in order.
    fn range(&self, start_time: u64, count: usize, out: &mut Vec<Sample>) -> Result<()>;

    /// Replaces existing samples: `(index, new sample with the same time)`.
    fn update(&mut self, updates: &[(u64, Sample)]) -> Result<()>;

    /// Inserts samples keeping the `time` order: `(index, sample)`, in
    /// descending index order so earlier positions do not shift.
    fn insert(&mut self, inserts: &[(u64, Sample)]) -> Result<()>;

    /// Removes samples: `(index, time)`, in descending index order.
    fn remove(&mut self, removals: &[(u64, u64)]) -> Result<()>;

    /// Bytes used on disk.
    fn disk_size(&self) -> Result<u64>;
}

/// Checks that two storages hold the same samples (count and checksum).
///
/// # Errors
///
/// Fails if a read fails or the storages differ.
pub fn check_same_data<A: Store, B: Store>(a: &A, b: &B) -> Result<()> {
    let (left, right) = (a.scan()?, b.scan()?);
    if left != right || a.len()? != b.len()? {
        anyhow::bail!(
            "{} and {} hold different data: {} rows (checksum {:x}) vs {} rows (checksum {:x})",
            A::NAME,
            B::NAME,
            left.count(),
            left.value(),
            right.count(),
            right.value()
        );
    }
    Ok(())
}
