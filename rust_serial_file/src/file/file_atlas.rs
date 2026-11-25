//! A thread-safe map of opened row files, each held under an exclusive
//! operating-system lock and shared through a [`FileLock`].
//!
//! [`FileAtlas`] owns every file it opens. Opening a file takes an exclusive
//! OS lock on it with [`File::try_lock`](std::fs::File::try_lock) and fails at
//! once if another process, or another handle in this process, holds it. The
//! OS lock is kept until the file is closed: when the [`FileLock`] returned by
//! [`FileAtlas::remove`] is dropped, or when the atlas (or its last handle to
//! the file) is dropped.
//!
//! On Unix the OS lock is advisory (`flock`): it only stops processes and
//! handles that also take a lock. On Windows it is a mandatory lock. On a
//! platform without file locking, opening fails.
//!
//! Access goes through handles (`Arc<FileLock>`) or closures:
//!
//! ```no_run
//! use rust_serial_file::{
//!     adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
//!     file::{file_atlas::FileAtlas, struct_file::StructFile},
//! };
//!
//! # fn main() -> anyhow::Result<()> {
//! let atlas = FileAtlas::<String>::new();
//! atlas.open_struct::<u32>("values".to_owned(), "values.bin", 0)?;
//!
//! atlas.with_struct_write::<u32, _, _>("values", |file| file.serial_write(7))?;
//!
//! let handle = atlas.get("values").unwrap();
//! let reader = StructFile::<u32, _>::new(handle.read()?)?;
//! assert_eq!(reader.serial_get(0)?, 7);
//! # Ok(())
//! # }
//! ```

use std::{
    borrow::Borrow,
    collections::{HashMap, hash_map},
    error::Error,
    fmt,
    fs::{OpenOptions, TryLockError},
    hash::Hash,
    iter::FusedIterator,
    path::Path,
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{Context, Result, bail};

use crate::serialized_struct::SerializedStruct;

use super::{
    file_lock::{FileLock, FileReadGuard, FileWriteGuard},
    file_metadata::FileMetadata,
    struct_file::StructFile,
};

/// Errors specific to [`FileAtlas`] operations.
///
/// They are returned inside [`anyhow::Error`]; use
/// `error.downcast_ref::<FileAtlasError>()` to tell them apart. They carry no
/// key, path, or file data.
#[derive(Debug, PartialEq, Eq)]
pub enum FileAtlasError {
    /// An entry with the same key is already in the atlas.
    KeyAlreadyPresent,
    /// No entry with the given key is in the atlas.
    KeyNotFound,
    /// The entry cannot be removed because other handles to it exist.
    InUse,
    /// The file is locked by another process or another handle.
    FileLocked,
}

impl fmt::Display for FileAtlasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::KeyAlreadyPresent => "an entry with this key is already in the file atlas",
            Self::KeyNotFound => "no entry with this key is in the file atlas",
            Self::InUse => "the file atlas entry is still in use by other handles",
            Self::FileLocked => "the file is locked by another process or handle",
        };
        formatter.write_str(message)
    }
}

impl Error for FileAtlasError {}

/// A thread-safe map from keys to opened row files.
///
/// Each entry is a [`FileLock`] stored in an [`Arc`]. The map has its own
/// [`RwLock`], held only for the map operation itself: never while a file is
/// locked and never while a closure passed to a `with_*` method runs. Work on
/// one file therefore never blocks work on another, and a closure may call
/// any atlas method.
///
/// Every file is held under an exclusive operating-system lock taken when it
/// is opened (see the [module documentation](self)). This lock is only
/// released when the file is closed.
///
/// The atlas is `Send` and `Sync` when `K` is, so it can be shared with
/// `Arc<FileAtlas<K>>`.
///
/// # Re-entrancy
///
/// Inside a `with_write` or `with_struct_write` closure, do not take another
/// guard on the same entry (through a handle or another `with_*` call): like
/// any [`FileLock`], this deadlocks or panics.
pub struct FileAtlas<K> {
    entries: RwLock<HashMap<K, Arc<FileLock>>>,
}

impl<K: Hash + Eq> FileAtlas<K> {
    /// Creates an empty atlas.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Opens or creates the file at `path`, locks it, and stores it under
    /// `key`. Returns a handle to the new entry.
    ///
    /// The file is opened for reading and writing, created if missing, and
    /// never truncated, as with [`FileMetadata::open_or_create`]. An exclusive
    /// operating-system lock is then taken with `File::try_lock`, without
    /// waiting. The lock is released only when the file is closed, that is
    /// when the [`FileLock`] returned by [`remove`](Self::remove), or the
    /// atlas holding it, is dropped.
    ///
    /// # Errors
    ///
    /// - [`FileAtlasError::KeyAlreadyPresent`] if `key` is already present;
    ///   the existing entry is not changed.
    /// - [`FileAtlasError::FileLocked`] if another process or handle holds
    ///   the file; no entry is added.
    /// - `element_size == 0`, open, lock, or metadata errors, with the path as
    ///   context. The file may already have been created on disk.
    pub fn open_or_create(
        &self,
        key: K,
        path: impl AsRef<Path>,
        element_size: u64,
        data_offset: u64,
    ) -> Result<Arc<FileLock>> {
        if self.read_entries().contains_key(&key) {
            return Err(FileAtlasError::KeyAlreadyPresent.into());
        }
        if element_size == 0 {
            bail!("element_size must be greater than zero");
        }

        // Opening and locking happen without the map lock, so a slow open
        // does not block other threads.
        let path = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open or create {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(FileAtlasError::FileLocked.into()),
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| format!("failed to lock {}", path.display()));
            }
        }
        let metadata = FileMetadata::new(file, element_size, data_offset)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let handle = Arc::new(FileLock::new(metadata));

        // Another thread may have inserted the key meanwhile; then the new
        // file is dropped here, which releases its OS lock.
        match self.write_entries().entry(key) {
            hash_map::Entry::Occupied(_) => Err(FileAtlasError::KeyAlreadyPresent.into()),
            hash_map::Entry::Vacant(entry) => Ok(Arc::clone(entry.insert(handle))),
        }
    }

    /// Opens or creates a file whose rows are `T` records, as
    /// [`open_or_create`](Self::open_or_create) with `element_size = T::SIZE`.
    ///
    /// # Errors
    ///
    /// As `open_or_create`, and also when `T::SIZE` is zero.
    pub fn open_struct<T: SerializedStruct>(
        &self,
        key: K,
        path: impl AsRef<Path>,
        data_offset: u64,
    ) -> Result<Arc<FileLock>> {
        let element_size =
            u64::try_from(T::SIZE).context("serialized row size does not fit in u64")?;
        self.open_or_create(key, path, element_size, data_offset)
    }

    /// Returns a new handle to the entry for `key`, or `None`.
    ///
    /// All handles to an entry point to the same [`FileLock`]. While any
    /// handle exists, the entry cannot be removed.
    pub fn get<Q>(&self, key: &Q) -> Option<Arc<FileLock>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.read_entries().get(key).map(Arc::clone)
    }

    /// Returns whether an entry for `key` exists.
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.read_entries().contains_key(key)
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.read_entries().len()
    }

    /// Returns whether the atlas has no entries.
    pub fn is_empty(&self) -> bool {
        self.read_entries().is_empty()
    }

    /// Returns a snapshot of the keys, in unspecified order.
    pub fn keys(&self) -> Vec<K>
    where
        K: Clone,
    {
        self.read_entries().keys().cloned().collect()
    }

    /// Returns an iterator over a snapshot of the entries, as
    /// `(key, handle)` pairs in unspecified order.
    ///
    /// The map lock is released before this returns, so the loop may call any
    /// atlas method; later opens and removals do not change the snapshot.
    /// The handles are ordinary handles: while the iterator, or a handle it
    /// yielded, is alive, [`remove`](Self::remove) of that entry returns
    /// [`FileAtlasError::InUse`].
    pub fn iter(&self) -> FileAtlasIter<K>
    where
        K: Clone,
    {
        let snapshot: Vec<_> = self
            .read_entries()
            .iter()
            .map(|(key, handle)| (key.clone(), Arc::clone(handle)))
            .collect();
        FileAtlasIter {
            inner: snapshot.into_iter(),
        }
    }

    /// Removes the entry for `key` and returns its owned [`FileLock`], if the
    /// atlas holds the only handle to it.
    ///
    /// The returned lock keeps the file open and its operating-system lock
    /// held until it is dropped. It can be inspected with
    /// [`FileLock::recover`] or turned into its [`FileMetadata`] with
    /// [`FileLock::into_inner`], even if it is poisoned.
    ///
    /// # Errors
    ///
    /// - [`FileAtlasError::KeyNotFound`] if there is no entry for `key`.
    /// - [`FileAtlasError::InUse`] if any other handle exists; the entry
    ///   stays in the atlas.
    pub fn remove<Q>(&self, key: &Q) -> Result<FileLock>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut entries = self.write_entries();
        let (owned_key, handle) = entries
            .remove_entry(key)
            .ok_or(FileAtlasError::KeyNotFound)?;
        // New handles are only cloned from the map, which is locked here, so
        // the count cannot grow from one while this guard is held.
        match Arc::try_unwrap(handle) {
            Ok(lock) => Ok(lock),
            Err(handle) => {
                entries.insert(owned_key, handle);
                Err(FileAtlasError::InUse.into())
            }
        }
    }

    /// Calls `f` with shared access to the file of `key`, holding its read
    /// guard only while `f` runs.
    ///
    /// # Errors
    ///
    /// [`FileAtlasError::KeyNotFound`], the [`FileLock`] poisoning error, or
    /// the error returned by `f`, unchanged.
    pub fn with_read<Q, R>(&self, key: &Q, f: impl FnOnce(&FileMetadata) -> Result<R>) -> Result<R>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let handle = self.handle(key)?;
        let guard = handle.read()?;
        f(&guard)
    }

    /// Calls `f` with exclusive access to the file of `key`, holding its
    /// write guard only while `f` runs.
    ///
    /// # Errors
    ///
    /// As [`with_read`](Self::with_read).
    pub fn with_write<Q, R>(
        &self,
        key: &Q,
        f: impl FnOnce(&mut FileMetadata) -> Result<R>,
    ) -> Result<R>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let handle = self.handle(key)?;
        let mut guard = handle.write()?;
        f(&mut guard)
    }

    /// Calls `f` with a read-only typed view of the file of `key`. The view
    /// holds the read guard while `f` runs.
    ///
    /// # Errors
    ///
    /// As [`with_read`](Self::with_read), and the [`StructFile`]
    /// construction error (for example a row width other than `T::SIZE`), in
    /// which case `f` is not called.
    pub fn with_struct_read<T, Q, R>(
        &self,
        key: &Q,
        f: impl FnOnce(&StructFile<T, FileReadGuard<'_>>) -> Result<R>,
    ) -> Result<R>
    where
        T: SerializedStruct,
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let handle = self.handle(key)?;
        let view = StructFile::new(handle.read()?)?;
        f(&view)
    }

    /// Calls `f` with a writable typed view of the file of `key`. The view
    /// holds the write guard while `f` runs.
    ///
    /// # Errors
    ///
    /// As [`with_struct_read`](Self::with_struct_read).
    pub fn with_struct_write<T, Q, R>(
        &self,
        key: &Q,
        f: impl FnOnce(&mut StructFile<T, FileWriteGuard<'_>>) -> Result<R>,
    ) -> Result<R>
    where
        T: SerializedStruct,
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let handle = self.handle(key)?;
        let mut view = StructFile::new(handle.write()?)?;
        f(&mut view)
    }

    fn handle<Q>(&self, key: &Q) -> Result<Arc<FileLock>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        Ok(self.get(key).ok_or(FileAtlasError::KeyNotFound)?)
    }

    // The map lock only guards `HashMap` operations and `K`'s `Hash`/`Eq`; the
    // entries are `Arc` handles, so a panic under the lock cannot leave a
    // `FileLock` half-updated. Poisoning is therefore recovered, not reported.
    fn read_entries(&self) -> RwLockReadGuard<'_, HashMap<K, Arc<FileLock>>> {
        self.entries.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_entries(&self) -> RwLockWriteGuard<'_, HashMap<K, Arc<FileLock>>> {
        self.entries.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<K: Hash + Eq> Default for FileAtlas<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq> IntoIterator for &FileAtlas<K> {
    type Item = (K, Arc<FileLock>);
    type IntoIter = FileAtlasIter<K>;

    /// Same snapshot as [`FileAtlas::iter`].
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K: Hash + Eq> IntoIterator for FileAtlas<K> {
    type Item = (K, Arc<FileLock>);
    type IntoIter = FileAtlasIntoIter<K>;

    /// Consumes the atlas and yields every entry. Each file, and its
    /// operating-system lock, is released when its last handle is dropped.
    fn into_iter(self) -> Self::IntoIter {
        let entries = self
            .entries
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        FileAtlasIntoIter {
            inner: entries.into_iter(),
        }
    }
}

/// Iterator over a snapshot of a [`FileAtlas`], returned by
/// [`FileAtlas::iter`].
pub struct FileAtlasIter<K> {
    inner: std::vec::IntoIter<(K, Arc<FileLock>)>,
}

impl<K> Iterator for FileAtlasIter<K> {
    type Item = (K, Arc<FileLock>);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K> ExactSizeIterator for FileAtlasIter<K> {}

impl<K> FusedIterator for FileAtlasIter<K> {}

/// Owning iterator over the entries of a consumed [`FileAtlas`].
pub struct FileAtlasIntoIter<K> {
    inner: hash_map::IntoIter<K, Arc<FileLock>>,
}

impl<K> Iterator for FileAtlasIntoIter<K> {
    type Item = (K, Arc<FileLock>);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K> ExactSizeIterator for FileAtlasIntoIter<K> {}

impl<K> FusedIterator for FileAtlasIntoIter<K> {}
