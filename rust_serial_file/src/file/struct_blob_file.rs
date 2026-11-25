//! A struct file and its blob file, owned and compacted together.

use std::{
    fs::{self, File, OpenOptions},
    marker::PhantomData,
    mem,
    path::Path,
};

use anyhow::{Context, Result};

use crate::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    serialized_struct::SerializedStruct,
};

use super::{
    blob_file::{BlobFields, BlobFile, BlobStats, compact_path, ensure_no_leftover_compaction},
    file_metadata::FileMetadata,
    struct_file::{StructFile, StructReadOnlyFile, StructWriteFile},
};

/// A struct file of `T` rows and the blob file holding their variable-length
/// values, opened and used together.
///
/// Rows reference values with [`BlobRef`](super::blob_file::BlobRef) fields.
/// Keeping rows and values consistent (write order, sync points) is the
/// caller's job; see the [`blob_file`](super::blob_file) module.
///
/// ```
/// use rust_serial_file::{
///     adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
///     file::{
///         blob_file::{BlobFields, BlobRef},
///         struct_blob_file::StructBlobFile,
///     },
///     serialized_struct,
/// };
/// use serialized_struct_derive::SerializedStruct;
///
/// #[derive(SerializedStruct)]
/// struct Sample {
///     time: u64,
///     name: BlobRef,
/// }
///
/// impl BlobFields for Sample {
///     fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef)) {
///         f(&mut self.name);
///     }
/// }
///
/// # fn main() -> anyhow::Result<()> {
/// # let directory = tempfile::tempdir()?;
/// # let rows_path = directory.path().join("samples.bin");
/// # let blob_path = directory.path().join("samples.blob");
/// let mut file = StructBlobFile::<Sample>::open_or_create(&rows_path, &blob_path, 0)?;
/// {
///     let (mut rows, blob) = file.parts_mut()?;
///     let name = blob.write_str("sensor A")?;
///     rows.serial_write(Sample { time: 1, name })?;
/// }
/// file.sync_data()?;
///
/// let (rows, blob) = file.parts()?;
/// assert_eq!(blob.read_string(rows.serial_get(0)?.name)?, "sensor A");
/// # Ok(())
/// # }
/// ```
pub struct StructBlobFile<T: SerializedStruct> {
    metadata: FileMetadata,
    blob: BlobFile,
    _record: PhantomData<T>,
}

impl<T: SerializedStruct> StructBlobFile<T> {
    /// Opens or creates the struct file at `path` (rows of `T`, after a
    /// `data_offset`-byte metadata prefix) and the blob file at `blob_path`.
    ///
    /// # Errors
    ///
    /// Fails if a `.compact` file of either path exists (an interrupted
    /// compaction), or as [`StructFile::open_or_create`] and
    /// [`BlobFile::open_or_create`].
    pub fn open_or_create(
        path: impl AsRef<Path>,
        blob_path: impl AsRef<Path>,
        data_offset: u64,
    ) -> Result<Self> {
        let path = path.as_ref();
        let blob_path = blob_path.as_ref();
        ensure_no_leftover_compaction(path)?;
        ensure_no_leftover_compaction(blob_path)?;
        let metadata = StructFile::<T>::open_or_create(path, data_offset)?.into_file_metadata();
        let blob = BlobFile::open_or_create(blob_path)?;
        Ok(Self {
            metadata,
            blob,
            _record: PhantomData,
        })
    }

    /// A read-only typed view of the rows.
    ///
    /// # Errors
    ///
    /// Fails if the row count cannot be read.
    pub fn rows(&self) -> Result<StructReadOnlyFile<'_, T>> {
        StructFile::new(&self.metadata)
    }

    /// The blob file.
    pub fn blob(&self) -> &BlobFile {
        &self.blob
    }

    /// Read-only views of the rows and the blob file.
    ///
    /// # Errors
    ///
    /// As [`rows`](Self::rows).
    pub fn parts(&self) -> Result<(StructReadOnlyFile<'_, T>, &BlobFile)> {
        Ok((StructFile::new(&self.metadata)?, &self.blob))
    }

    /// Writable views of the rows and the blob file, usable together.
    ///
    /// # Errors
    ///
    /// As [`rows`](Self::rows).
    pub fn parts_mut(&mut self) -> Result<(StructWriteFile<'_, T>, &mut BlobFile)> {
        Ok((StructFile::new(&mut self.metadata)?, &mut self.blob))
    }

    /// Syncs the blob file, then the struct file (`sync_data`), so rows
    /// never reach storage before the values they reference.
    ///
    /// # Errors
    ///
    /// The first sync error; the struct file is not synced if the blob file
    /// failed.
    pub fn sync_data(&mut self) -> Result<()> {
        self.blob.sync_data()?;
        self.metadata.sync_data()
    }

    /// Like [`sync_data`](Self::sync_data), with `sync_all`.
    ///
    /// # Errors
    ///
    /// As [`sync_data`](Self::sync_data).
    pub fn sync_all(&mut self) -> Result<()> {
        self.blob.sync_all()?;
        self.metadata.sync_all()
    }

    /// Consumes the pair and returns the struct file metadata and the blob
    /// file.
    pub fn into_parts(self) -> (FileMetadata, BlobFile) {
        (self.metadata, self.blob)
    }
}

impl<T: SerializedStruct + BlobFields> StructBlobFile<T> {
    /// Rewrites the blob file without holes, in row order, and updates every
    /// row's [`BlobRef`](super::blob_file::BlobRef)s. Values no row
    /// references (leaked by a crash) are dropped. See [`compact_files`] for
    /// the steps and the recovery of an interrupted compaction.
    ///
    /// Returns the statistics of the new blob file.
    ///
    /// # Errors
    ///
    /// As [`compact_files`].
    pub fn compact(&mut self) -> Result<BlobStats> {
        compact_files::<T>(&mut self.metadata, &mut self.blob, false)
    }
}

/// Compacts a struct file and its blob file.
///
/// Steps:
///
/// 1. Write a new struct file `<path>.compact` (same metadata prefix) and a
///    new blob file `<blob path>.compact`: each row is read, each referenced
///    value copied to the new blob file (each reference gets its own copy),
///    and the row written with the new references.
/// 2. Sync both new files.
/// 3. Close the old files (the handles now point to the new files) and
///    rename `<path>.compact` over `path`, then `<blob path>.compact` over
///    the blob path.
///
/// Until step 3 the original files are untouched, and a failure removes the
/// `.compact` files. An interruption during step 3 leaves the files to be
/// recovered by hand, which opening reports (a `.compact` file exists):
///
/// - both `.compact` files exist: the renames did not start; delete both;
/// - only `<blob path>.compact` exists: the struct file is already the new
///   one; rename `<blob path>.compact` over the blob path to finish.
///
/// With `lock_files`, the new files are locked like `FileAtlas` files.
///
/// # Errors
///
/// Fails if either handle has no path, if a `.compact` file already exists,
/// on any read, decode, write or sync error, or if a rename fails (the error
/// says which state the files are in).
pub(crate) fn compact_files<T: SerializedStruct + BlobFields>(
    metadata: &mut FileMetadata,
    blob: &mut BlobFile,
    lock_files: bool,
) -> Result<BlobStats> {
    let struct_path = metadata
        .path()
        .context("cannot compact: the struct file was not opened from a path")?
        .to_owned();
    let blob_path = blob
        .path()
        .context("cannot compact: the blob file was not opened from a path")?
        .to_owned();
    ensure_no_leftover_compaction(&struct_path)?;
    ensure_no_leftover_compaction(&blob_path)?;
    let struct_tmp = compact_path(&struct_path);
    let blob_tmp = compact_path(&blob_path);

    let (new_metadata, new_blob) =
        match write_compacted::<T>(metadata, blob, &struct_tmp, &blob_tmp, lock_files) {
            Ok(files) => files,
            Err(error) => {
                // The new handles are already closed here; the originals are
                // untouched. Best-effort cleanup of the partial files.
                let _ = fs::remove_file(&struct_tmp);
                let _ = fs::remove_file(&blob_tmp);
                return Err(error.context("compaction failed; the original files are unchanged"));
            }
        };

    // Close the old files before replacing them (required on Windows).
    drop(mem::replace(metadata, new_metadata));
    drop(mem::replace(blob, new_blob));

    fs::rename(&struct_tmp, &struct_path).with_context(|| {
        format!(
            "compaction: could not rename {} over {}; the original files are unchanged on disk, \
             delete both .compact files before reopening",
            struct_tmp.display(),
            struct_path.display()
        )
    })?;
    metadata.set_path(struct_path);
    fs::rename(&blob_tmp, &blob_path).with_context(|| {
        format!(
            "compaction: could not rename {} over {}; the struct file is already compacted, \
             rename the blob file by hand to finish",
            blob_tmp.display(),
            blob_path.display()
        )
    })?;
    blob.set_path(blob_path);
    Ok(blob.stats())
}

/// Steps 1 and 2 of [`compact_files`]: writes and syncs the `.compact` files
/// and returns their handles.
fn write_compacted<T: SerializedStruct + BlobFields>(
    metadata: &FileMetadata,
    blob: &BlobFile,
    struct_tmp: &Path,
    blob_tmp: &Path,
    lock_files: bool,
) -> Result<(FileMetadata, BlobFile)> {
    let mut new_metadata = FileMetadata::new(
        create_new(struct_tmp, lock_files)?,
        metadata.element_size(),
        metadata.data_offset(),
    )?;
    new_metadata.set_path(struct_tmp.to_owned());
    if metadata.data_offset() > 0 {
        let mut prefix = vec![
            0;
            usize::try_from(metadata.data_offset())
                .context("metadata prefix does not fit in memory")?
        ];
        metadata.get_raw_metadata(&mut prefix)?;
        new_metadata.update_raw_metadata(&prefix)?;
    }
    let mut new_blob = BlobFile::new(create_new(blob_tmp, lock_files)?)?;
    new_blob.set_path(blob_tmp.to_owned());

    {
        let rows = StructFile::<T, _>::new(metadata)?;
        let mut new_rows = StructFile::<T, _>::new(&mut new_metadata)?;
        for row in rows.serial_iter()? {
            let mut row = row?;
            let mut failure = None;
            row.blob_refs_mut(&mut |reference| {
                if failure.is_some() || reference.is_empty() {
                    return;
                }
                match blob
                    .read(*reference)
                    .and_then(|bytes| new_blob.write(&bytes))
                {
                    Ok(copied) => *reference = copied,
                    Err(error) => failure = Some(error),
                }
            });
            if let Some(error) = failure {
                return Err(error);
            }
            new_rows.serial_write(row)?;
        }
    }
    new_blob.sync_data()?;
    new_metadata.sync_data()?;
    Ok((new_metadata, new_blob))
}

/// Creates a new file (failing if it exists), locked if requested.
fn create_new(path: &Path, lock: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    if lock {
        // The file was just created, so nothing else can hold its lock.
        file.try_lock()
            .with_context(|| format!("failed to lock {}", path.display()))?;
    }
    Ok(file)
}
