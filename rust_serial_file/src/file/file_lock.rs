//! In-process reader-writer lock around a [`FileMetadata`].
//!
//! [`FileLock`] owns a [`FileMetadata`] and lets threads borrow it either
//! through many concurrent [`FileReadGuard`]s or through one exclusive
//! [`FileWriteGuard`]. The lock is held until the guard is dropped.
//!
//! # In-process locking only
//!
//! `FileLock` coordinates only the threads that share the same `FileLock`
//! (typically through an `Arc<FileLock>`). It does **not** take any
//! operating-system file lock and does not know about other processes or
//! other handles opened on the same path. Operating-system locking is expected
//! to be handled by the component that owns the opened files (for example
//! [`FileAtlas`](super::file_atlas::FileAtlas)). When `FileLock` is used
//! without such a component, the caller must make sure that no other process
//! or handle reads or writes the file while it is in use.
//!
//! # Poisoning and recovery
//!
//! If a thread panics while holding a [`FileWriteGuard`], the lock is
//! poisoned: every acquisition returns an error until [`FileLock::recover`] is
//! called. `recover` returns a [`PoisonReport`] comparing the file state when
//! the write guard was acquired with the state when the panic dropped it, and
//! clears the poison. It does not repair anything: the caller decides whether
//! to finish the interrupted work, undo it, or drop the lock and reopen the
//! file from disk.

use std::{
    borrow::{Borrow, BorrowMut},
    ops::{Deref, DerefMut},
    path::Path,
    sync::{
        PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use anyhow::{Result, anyhow};

use super::file_metadata::FileMetadata;

const POISONED_MESSAGE: &str = "file lock is poisoned: a thread panicked while holding the write guard; \
     call FileLock::recover to inspect the interrupted write and clear the poison";

/// File state captured for a [`PoisonReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    /// Physical file length in bytes.
    pub file_len: u64,
    /// Number of complete rows, as reported by [`FileMetadata::element_count`].
    pub element_count: u64,
    /// Length of the metadata prefix, as reported by [`FileMetadata::data_offset`].
    pub data_offset: u64,
}

impl FileSnapshot {
    fn capture(metadata: &FileMetadata) -> Result<Self> {
        Ok(Self {
            file_len: metadata.file_len()?,
            element_count: metadata.element_count()?,
            data_offset: metadata.data_offset(),
        })
    }
}

/// Description of a write session interrupted by a panic.
///
/// The report only describes changes in file length, element count, and data
/// offset. Rows overwritten in place (same length and count) are **not**
/// detected: `before == after` does not prove that the file is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoisonReport {
    /// File state when the write guard was acquired.
    pub before: FileSnapshot,
    /// File state when the panic dropped the write guard, or `None` if it
    /// could not be read.
    pub after: Option<FileSnapshot>,
    /// Name of the panicking thread, or `None` if the thread is unnamed.
    pub thread_name: Option<String>,
}

/// Value protected by the `RwLock`. Keeping the report next to the metadata
/// lets the write guard store it without taking another lock.
struct Shared {
    metadata: FileMetadata,
    poison_report: Option<PoisonReport>,
}

/// Owns a [`FileMetadata`] behind an in-process reader-writer lock.
///
/// Any number of [`FileReadGuard`]s can be alive at the same time, or one
/// [`FileWriteGuard`], never both. `FileLock` is `Send + Sync` and is meant to
/// be shared with `Arc<FileLock>`.
///
/// This lock takes **no operating-system file lock**: it only coordinates the
/// threads that share this `FileLock`. Protecting the file against other
/// processes or handles is the job of the component that owns the opened
/// files (for example [`FileAtlas`](super::file_atlas::FileAtlas)); without
/// it, the caller must guarantee that nothing else accesses the file.
pub struct FileLock {
    inner: RwLock<Shared>,
    generation: AtomicU64,
}

impl FileLock {
    /// Takes ownership of `metadata` and returns an unlocked `FileLock`.
    pub fn new(metadata: FileMetadata) -> Self {
        Self {
            inner: RwLock::new(Shared {
                metadata,
                poison_report: None,
            }),
            generation: AtomicU64::new(0),
        }
    }

    /// Opens or creates the file with [`FileMetadata::open_or_create`] and
    /// wraps it. Errors from `FileMetadata` are returned unchanged.
    pub fn open_or_create(
        path: impl AsRef<Path>,
        element_size: u64,
        data_offset: u64,
    ) -> Result<Self> {
        FileMetadata::open_or_create(path, element_size, data_offset).map(Self::new)
    }

    /// Acquires a shared read guard, blocking while a write guard is alive.
    ///
    /// Only threads sharing this `FileLock` are coordinated; no
    /// operating-system file lock is taken.
    ///
    /// Calling `write` (or `recover`) on the same thread while this guard is
    /// alive may deadlock or panic, as with [`std::sync::RwLock`].
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    pub fn read(&self) -> Result<FileReadGuard<'_>> {
        let guard = self.inner.read().map_err(poisoned)?;
        Ok(FileReadGuard { guard })
    }

    /// Acquires the exclusive write guard, blocking while any read or write
    /// guard is alive.
    ///
    /// Only threads sharing this `FileLock` are coordinated; no
    /// operating-system file lock is taken.
    ///
    /// Calling `read` or `write` on the same thread while this guard is alive
    /// may deadlock or panic, as with [`std::sync::RwLock`].
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned, or if the file length cannot
    /// be read for the [`FileSnapshot`] taken at acquisition.
    pub fn write(&self) -> Result<FileWriteGuard<'_>> {
        let guard = self.inner.write().map_err(poisoned)?;
        FileWriteGuard::new(guard, &self.generation)
    }

    /// Tries to acquire a read guard without blocking.
    ///
    /// Returns `Ok(None)` while a write guard is alive.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    pub fn try_read(&self) -> Result<Option<FileReadGuard<'_>>> {
        match self.inner.try_read() {
            Ok(guard) => Ok(Some(FileReadGuard { guard })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(error)) => Err(poisoned(error)),
        }
    }

    /// Tries to acquire the write guard without blocking.
    ///
    /// Returns `Ok(None)` while any read or write guard is alive.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned, or if the file length cannot
    /// be read for the [`FileSnapshot`] taken at acquisition.
    pub fn try_write(&self) -> Result<Option<FileWriteGuard<'_>>> {
        match self.inner.try_write() {
            Ok(guard) => FileWriteGuard::new(guard, &self.generation).map(Some),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(error)) => Err(poisoned(error)),
        }
    }

    /// Returns mutable access without locking; the exclusive borrow of `self`
    /// already proves that no guard is alive.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned.
    pub fn get_mut(&mut self) -> Result<&mut FileMetadata> {
        let shared = self.inner.get_mut().map_err(poisoned)?;
        Ok(&mut shared.metadata)
    }

    /// Consumes the lock and returns the owned [`FileMetadata`].
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is poisoned; call [`FileLock::recover`]
    /// first.
    pub fn into_inner(self) -> Result<FileMetadata> {
        let shared = self.inner.into_inner().map_err(poisoned)?;
        Ok(shared.metadata)
    }

    /// Inspects and clears a poisoned lock. Blocks like [`FileLock::write`].
    ///
    /// On a poisoned lock, returns `Ok(Some(report))` describing the
    /// interrupted write session and clears the poison, so later calls behave
    /// as on a lock that was never poisoned. On a healthy lock, returns
    /// `Ok(None)` and changes nothing.
    ///
    /// `recover` does not repair or change the file or the [`FileMetadata`].
    /// The report does not detect rows overwritten in place; see
    /// [`PoisonReport`]. If the state looks wrong, drop the lock and reopen the
    /// path with [`FileMetadata::open_or_create`], which rebuilds the cached
    /// counts from the file length.
    ///
    /// The `Result` is kept for API stability; the current implementation
    /// does not fail.
    pub fn recover(&self) -> Result<Option<PoisonReport>> {
        match self.inner.write() {
            Ok(_) => Ok(None),
            Err(error) => {
                let mut shared = error.into_inner();
                let report = shared.poison_report.take();
                // Clear while still holding the guard so no other thread can
                // observe the lock as healthy with a stale report.
                self.inner.clear_poison();
                drop(shared);
                Ok(report)
            }
        }
    }

    /// Number of write guards released normally (not by a panic) since the
    /// lock was created. Read guards, [`FileLock::get_mut`], and write guards
    /// dropped by a panic do not change it. Readable even when poisoned.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

impl From<FileMetadata> for FileLock {
    fn from(metadata: FileMetadata) -> Self {
        Self::new(metadata)
    }
}

/// Shared borrow of the [`FileMetadata`] owned by a [`FileLock`].
///
/// The read lock is released when the guard is dropped.
pub struct FileReadGuard<'a> {
    guard: RwLockReadGuard<'a, Shared>,
}

impl Deref for FileReadGuard<'_> {
    type Target = FileMetadata;

    fn deref(&self) -> &FileMetadata {
        &self.guard.metadata
    }
}

/// Lets the guard be used wherever a `Borrow<FileMetadata>` holder is
/// expected, for example as the storage of a typed read-only view, which then
/// keeps the read lock for its whole lifetime. Returns the same metadata as
/// [`Deref`].
impl Borrow<FileMetadata> for FileReadGuard<'_> {
    fn borrow(&self) -> &FileMetadata {
        self
    }
}

/// Exclusive borrow of the [`FileMetadata`] owned by a [`FileLock`].
///
/// The write lock is released when the guard is dropped. If it is dropped by
/// a panic, the lock becomes poisoned and a [`PoisonReport`] is stored for
/// [`FileLock::recover`].
pub struct FileWriteGuard<'a> {
    guard: RwLockWriteGuard<'a, Shared>,
    generation: &'a AtomicU64,
    before: FileSnapshot,
    panicking_at_acquire: bool,
}

impl<'a> FileWriteGuard<'a> {
    fn new(guard: RwLockWriteGuard<'a, Shared>, generation: &'a AtomicU64) -> Result<Self> {
        // On error the raw std guard is dropped without running our `Drop`,
        // so the generation counter is not increased.
        let before = FileSnapshot::capture(&guard.metadata)?;
        Ok(Self {
            guard,
            generation,
            before,
            panicking_at_acquire: thread::panicking(),
        })
    }
}

impl Deref for FileWriteGuard<'_> {
    type Target = FileMetadata;

    fn deref(&self) -> &FileMetadata {
        &self.guard.metadata
    }
}

impl DerefMut for FileWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut FileMetadata {
        &mut self.guard.metadata
    }
}

/// Lets the guard be used wherever a `Borrow<FileMetadata>` holder is
/// expected, for example as the storage of a typed view, which then keeps the
/// write lock for its whole lifetime. Returns the same metadata as [`Deref`].
impl Borrow<FileMetadata> for FileWriteGuard<'_> {
    fn borrow(&self) -> &FileMetadata {
        self
    }
}

/// Mutable counterpart of the [`Borrow`] impl, for writable typed views.
/// Returns the same metadata as [`DerefMut`]; dropping the guard afterwards
/// updates the generation counter or stores a [`PoisonReport`] as usual.
impl BorrowMut<FileMetadata> for FileWriteGuard<'_> {
    fn borrow_mut(&mut self) -> &mut FileMetadata {
        self
    }
}

impl Drop for FileWriteGuard<'_> {
    // Must never panic: it can run during unwinding, where a second panic
    // aborts the process.
    fn drop(&mut self) {
        // Mirror the std rule: the inner guard poisons the lock only when a
        // panic started after it was acquired.
        let poisons_lock = thread::panicking() && !self.panicking_at_acquire;
        if poisons_lock {
            let after = FileSnapshot::capture(&self.guard.metadata).ok();
            let thread_name = thread::current().name().map(str::to_owned);
            self.guard.poison_report = Some(PoisonReport {
                before: self.before.clone(),
                after,
                thread_name,
            });
        } else {
            self.generation.fetch_add(1, Ordering::Release);
        }
    }
}

fn poisoned<T>(_: PoisonError<T>) -> anyhow::Error {
    anyhow!(POISONED_MESSAGE)
}
