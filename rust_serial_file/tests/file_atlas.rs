use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions, TryLockError},
    hash::{Hash, Hasher},
    iter::FusedIterator,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{Arc, Barrier, mpsc},
    thread,
};

use rust_serial_file::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    file::{
        file_atlas::{FileAtlas, FileAtlasError, FileAtlasIntoIter, FileAtlasIter},
        file_metadata::FileMetadata,
        struct_file::StructFile,
    },
    serialized_struct::{DecodeRaw, EncodeRaw, SerializedStruct},
};
use tempfile::{TempDir, tempdir};

const ROW_SIZE: u64 = 4;

fn path_in(directory: &TempDir, name: &str) -> PathBuf {
    directory.path().join(name)
}

fn row(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

fn read_value(metadata: &FileMetadata, index: u64) -> u32 {
    let mut buffer = [0u8; 4];
    metadata.read_row(index, &mut buffer).unwrap();
    u32::from_le_bytes(buffer)
}

fn atlas_error<T>(result: anyhow::Result<T>) -> FileAtlasError {
    let Err(error) = result else {
        panic!("expected a FileAtlasError, got Ok");
    };
    match error.downcast_ref::<FileAtlasError>() {
        Some(FileAtlasError::KeyAlreadyPresent) => FileAtlasError::KeyAlreadyPresent,
        Some(FileAtlasError::KeyNotFound) => FileAtlasError::KeyNotFound,
        Some(FileAtlasError::InUse) => FileAtlasError::InUse,
        Some(FileAtlasError::FileLocked) => FileAtlasError::FileLocked,
        None => panic!("expected a FileAtlasError, got: {error:#}"),
    }
}

/// Returns whether a plain handle on `path` can take the exclusive OS lock
/// right now. The probe handle is closed before returning.
fn can_lock(path: &Path) -> bool {
    let file = OpenOptions::new().read(true).open(path).unwrap();
    match file.try_lock() {
        Ok(()) => true,
        Err(TryLockError::WouldBlock) => false,
        Err(TryLockError::Error(error)) => panic!("unexpected lock error: {error}"),
    }
}

fn atlas_with_rows(directory: &TempDir, key: &str, count: u32) -> FileAtlas<String> {
    let atlas = FileAtlas::new();
    atlas
        .open_or_create(key.to_owned(), path_in(directory, key), ROW_SIZE, 0)
        .unwrap();
    atlas
        .with_write(key, |file| {
            for value in 0..count {
                file.append_row(&row(value))?;
            }
            Ok(())
        })
        .unwrap();
    atlas
}

struct ZeroSize;

impl EncodeRaw for ZeroSize {
    fn encode(&self, _data: &mut [u8]) -> usize {
        0
    }
}

impl DecodeRaw for ZeroSize {
    fn from_le_bytes_slices(_data: &[u8], _offset: &mut usize) -> Self {
        Self
    }
}

impl SerializedStruct for ZeroSize {
    const SIZE: usize = 0;
}

// T-001
#[test]
fn new_and_default_atlas_are_empty() {
    for atlas in [FileAtlas::<String>::new(), FileAtlas::default()] {
        assert_eq!(atlas.len(), 0);
        assert!(atlas.is_empty());
        assert!(atlas.keys().is_empty());
        assert!(atlas.get("missing").is_none());
        assert!(!atlas.contains("missing"));
    }
}

// T-002
#[test]
fn open_or_create_stores_and_shares_handles() {
    let directory = tempdir().unwrap();
    let existing = path_in(&directory, "existing.bin");
    {
        let mut metadata = FileMetadata::open_or_create(&existing, ROW_SIZE, 0).unwrap();
        metadata.append_row(&row(1)).unwrap();
        metadata.append_row(&row(2)).unwrap();
    }
    let fresh = path_in(&directory, "fresh.bin");
    let atlas = FileAtlas::new();

    let fresh_handle = atlas
        .open_or_create("fresh".to_owned(), &fresh, ROW_SIZE, 0)
        .unwrap();
    let existing_handle = atlas
        .open_or_create("existing".to_owned(), &existing, ROW_SIZE, 0)
        .unwrap();

    assert_eq!(fs::metadata(&fresh).unwrap().len(), 0);
    assert_eq!(fs::metadata(&existing).unwrap().len(), 8);
    assert_eq!(existing_handle.read().unwrap().element_count().unwrap(), 2);
    assert!(Arc::ptr_eq(&fresh_handle, &atlas.get("fresh").unwrap()));
    assert!(Arc::ptr_eq(
        &atlas.get("existing").unwrap(),
        &atlas.get("existing").unwrap()
    ));
    assert_eq!(atlas.len(), 2);
    let keys: HashSet<_> = atlas.keys().into_iter().collect();
    assert_eq!(
        keys,
        HashSet::from(["fresh".to_owned(), "existing".to_owned()])
    );
}

// T-003
#[test]
fn opening_an_existing_key_is_rejected() {
    let directory = tempdir().unwrap();
    let atlas = FileAtlas::new();
    let original = atlas
        .open_or_create("key".to_owned(), path_in(&directory, "a.bin"), ROW_SIZE, 0)
        .unwrap();
    let other = path_in(&directory, "b.bin");

    let error = atlas_error(atlas.open_or_create("key".to_owned(), &other, ROW_SIZE, 0));

    assert_eq!(error, FileAtlasError::KeyAlreadyPresent);
    assert!(!other.exists());
    assert!(Arc::ptr_eq(&original, &atlas.get("key").unwrap()));
}

// T-004
#[test]
fn opening_a_locked_path_is_rejected() {
    let directory = tempdir().unwrap();
    let path = path_in(&directory, "shared.bin");
    let atlas = FileAtlas::new();
    atlas
        .open_or_create("first".to_owned(), &path, ROW_SIZE, 0)
        .unwrap();

    let error = atlas_error(atlas.open_or_create("second".to_owned(), &path, ROW_SIZE, 0));

    assert_eq!(error, FileAtlasError::FileLocked);
    assert!(!atlas.contains("second"));
    assert_eq!(atlas.len(), 1);
}

// T-005
#[test]
fn os_lock_is_held_until_removed_lock_is_dropped() {
    let directory = tempdir().unwrap();
    let path = path_in(&directory, "locked.bin");
    let atlas = FileAtlas::new();
    atlas
        .open_or_create("key".to_owned(), &path, ROW_SIZE, 0)
        .unwrap();
    assert!(!can_lock(&path));

    let lock = atlas.remove("key").unwrap();
    assert!(!can_lock(&path));
    drop(lock);
    assert!(can_lock(&path));
}

// T-006
#[test]
fn open_struct_uses_the_record_size() {
    let directory = tempdir().unwrap();
    let atlas = FileAtlas::new();
    let handle = atlas
        .open_struct::<u64>("values".to_owned(), path_in(&directory, "values.bin"), 0)
        .unwrap();
    assert_eq!(handle.read().unwrap().element_size(), 8);

    {
        let mut writer = StructFile::<u64, _>::new(handle.write().unwrap()).unwrap();
        writer.serial_write(42).unwrap();
    }
    let reader = StructFile::<u64, _>::new(handle.read().unwrap()).unwrap();
    assert_eq!(reader.serial_get(0).unwrap(), 42);

    let zero = path_in(&directory, "zero.bin");
    assert!(
        atlas
            .open_struct::<ZeroSize>("zero".to_owned(), &zero, 0)
            .is_err()
    );
    assert!(!atlas.contains("zero"));
    assert!(!zero.exists());
}

// T-007
#[test]
fn remove_returns_the_owned_file_lock() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_rows(&directory, "key", 3);

    let lock = atlas.remove("key").unwrap();
    let metadata = lock.into_inner().unwrap();

    assert_eq!(metadata.element_count().unwrap(), 3);
    assert_eq!(read_value(&metadata, 2), 2);
    assert!(!atlas.contains("key"));
    atlas
        .open_or_create("key".to_owned(), path_in(&directory, "other"), ROW_SIZE, 0)
        .unwrap();
}

// T-008
#[test]
fn remove_fails_while_a_handle_is_held() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_rows(&directory, "key", 1);
    let handle = atlas.get("key").unwrap();

    assert_eq!(atlas_error(atlas.remove("key")), FileAtlasError::InUse);
    assert!(atlas.contains("key"));

    drop(handle);
    assert!(atlas.remove("key").is_ok());
}

// T-009
#[test]
fn lookups_accept_borrowed_keys() {
    let directory = tempdir().unwrap();
    let atlas: FileAtlas<String> = atlas_with_rows(&directory, "key", 2);
    let key: &str = "key";

    assert!(atlas.get(key).is_some());
    assert!(atlas.contains(key));
    assert_eq!(
        atlas.with_read(key, |file| file.element_count()).unwrap(),
        2
    );
    assert!(atlas.remove(key).is_ok());
}

// T-010
#[test]
fn atlas_errors_display_and_downcast() {
    let cases = [
        (
            FileAtlasError::KeyAlreadyPresent,
            "an entry with this key is already in the file atlas",
        ),
        (
            FileAtlasError::KeyNotFound,
            "no entry with this key is in the file atlas",
        ),
        (
            FileAtlasError::InUse,
            "the file atlas entry is still in use by other handles",
        ),
        (
            FileAtlasError::FileLocked,
            "the file is locked by another process or handle",
        ),
    ];

    for (variant, message) in cases {
        assert_eq!(variant.to_string(), message);
        assert!(std::error::Error::source(&variant).is_none());
        let expected = format!("{variant:?}");
        let error = anyhow::Error::new(variant);
        assert_eq!(
            format!("{:?}", error.downcast_ref::<FileAtlasError>().unwrap()),
            expected
        );
    }
}

// T-011
#[test]
fn remove_reports_missing_keys_and_returns_poisoned_locks() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_rows(&directory, "key", 1);
    assert_eq!(
        atlas_error(atlas.remove("missing")),
        FileAtlasError::KeyNotFound
    );

    let handle = atlas.get("key").unwrap();
    let result = thread::spawn(move || {
        let mut guard = handle.write().unwrap();
        guard.append_row(&row(9)).unwrap();
        panic!("intentional panic while holding the write guard");
    })
    .join();
    assert!(result.is_err());
    assert!(atlas.with_read("key", |_| Ok(())).is_err());

    let lock = atlas.remove("key").unwrap();
    let report = lock.recover().unwrap().unwrap();
    assert_eq!(report.before.element_count, 1);
    assert_eq!(report.after.unwrap().element_count, 2);
}

// T-012
#[test]
fn metadata_closures_read_write_and_propagate_errors() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_rows(&directory, "key", 0);

    atlas
        .with_write("key", |file| {
            file.append_row(&row(5))?;
            file.append_row(&row(6))
        })
        .unwrap();
    let values = atlas
        .with_read("key", |file| Ok((read_value(file, 0), read_value(file, 1))))
        .unwrap();
    assert_eq!(values, (5, 6));

    let error = atlas
        .with_read("key", |_| -> anyhow::Result<()> {
            anyhow::bail!("closure failed")
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "closure failed");
    assert_eq!(
        atlas_error(atlas.with_read("missing", |_| Ok(()))),
        FileAtlasError::KeyNotFound
    );
    assert_eq!(
        atlas_error(atlas.with_write("missing", |_| Ok(()))),
        FileAtlasError::KeyNotFound
    );
}

// T-013
#[test]
fn struct_closures_read_write_and_check_row_width() {
    let directory = tempdir().unwrap();
    let atlas = FileAtlas::new();
    atlas
        .open_struct::<u32>("values".to_owned(), path_in(&directory, "values.bin"), 0)
        .unwrap();

    atlas
        .with_struct_write::<u32, _, _>("values", |file| {
            file.serial_write(1)?;
            file.serial_write(3)?;
            file.serial_insert(1, 2)
        })
        .unwrap();
    let values: Vec<u32> = atlas
        .with_struct_read::<u32, _, _>("values", |file| file.serial_iter()?.collect())
        .unwrap();
    assert_eq!(values, [1, 2, 3]);

    let mut called = false;
    assert!(
        atlas
            .with_struct_read::<u64, _, _>("values", |_| {
                called = true;
                Ok(())
            })
            .is_err()
    );
    assert!(
        atlas
            .with_struct_write::<u64, _, _>("values", |_| {
                called = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!called);
    assert_eq!(
        atlas_error(atlas.with_struct_read::<u32, _, _>("missing", |_| Ok(()))),
        FileAtlasError::KeyNotFound
    );
}

// T-014
#[test]
fn closures_can_use_the_atlas_without_deadlock() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_rows(&directory, "a", 1);

    atlas
        .with_write("a", |file| {
            file.append_row(&row(1))?;
            atlas.open_or_create("b".to_owned(), path_in(&directory, "b"), ROW_SIZE, 0)?;
            assert!(atlas.get("a").is_some());
            assert!(atlas.get("b").is_some());
            assert!(atlas.contains("b"));
            assert_eq!(atlas.len(), 2);
            assert_eq!(atlas.keys().len(), 2);
            assert_eq!(atlas.with_read("b", |other| other.element_count())?, 0);
            Ok(())
        })
        .unwrap();

    assert_eq!(
        atlas.with_read("a", |file| file.element_count()).unwrap(),
        2
    );
}

// T-015
#[test]
fn running_closure_blocks_remove() {
    let directory = tempdir().unwrap();
    let atlas = Arc::new(atlas_with_rows(&directory, "key", 1));
    let inside = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));

    let reader = {
        let atlas = Arc::clone(&atlas);
        let inside = Arc::clone(&inside);
        let release = Arc::clone(&release);
        thread::spawn(move || {
            atlas
                .with_read("key", |_| {
                    inside.wait();
                    release.wait();
                    Ok(())
                })
                .unwrap();
        })
    };

    inside.wait();
    assert_eq!(atlas_error(atlas.remove("key")), FileAtlasError::InUse);
    release.wait();
    reader.join().unwrap();
    assert!(atlas.remove("key").is_ok());
}

// T-016
#[test]
fn entries_are_locked_independently_across_threads() {
    const READERS: usize = 4;
    let directory = tempdir().unwrap();
    let atlas = Arc::new(FileAtlas::<u32>::new());
    for key in [1, 2] {
        atlas
            .open_or_create(key, path_in(&directory, &key.to_string()), ROW_SIZE, 0)
            .unwrap();
    }
    atlas
        .with_write(&2, |file| file.append_row(&row(20)))
        .unwrap();

    let (locked_sender, locked) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel::<()>();
    let holder = {
        let handle = atlas.get(&1).unwrap();
        thread::spawn(move || {
            let _guard = handle.write().unwrap();
            locked_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
        })
    };
    locked.recv().unwrap();

    // Key 1 is write-locked from here until `release` is sent.
    let barrier = Arc::new(Barrier::new(READERS));
    let readers: Vec<_> = (0..READERS)
        .map(|_| {
            let atlas = Arc::clone(&atlas);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                atlas
                    .with_read(&2, |file| {
                        barrier.wait();
                        Ok(read_value(file, 0))
                    })
                    .unwrap()
            })
        })
        .collect();
    for reader in readers {
        assert_eq!(reader.join().unwrap(), 20);
    }
    atlas
        .with_write(&2, |file| file.append_row(&row(21)))
        .unwrap();
    atlas
        .open_or_create(3, path_in(&directory, "3"), ROW_SIZE, 0)
        .unwrap();
    assert_eq!(atlas.len(), 3);
    assert_eq!(atlas.keys().len(), 3);

    release.send(()).unwrap();
    holder.join().unwrap();
    assert!(atlas.remove(&3).is_ok());
}

// T-017
#[test]
fn racing_opens_of_one_key_have_one_winner() {
    const THREADS: usize = 6;
    let directory = Arc::new(tempdir().unwrap());
    let atlas = Arc::new(FileAtlas::<String>::new());
    let barrier = Arc::new(Barrier::new(THREADS));

    let openers: Vec<_> = (0..THREADS)
        .map(|index| {
            let atlas = Arc::clone(&atlas);
            let barrier = Arc::clone(&barrier);
            let path = path_in(&directory, &format!("race-{index}"));
            thread::spawn(move || {
                barrier.wait();
                let result = atlas.open_or_create("race".to_owned(), &path, ROW_SIZE, 0);
                (path, result.map(drop))
            })
        })
        .collect();

    let mut winners = 0;
    let mut loser_paths = Vec::new();
    for opener in openers {
        let (path, result) = opener.join().unwrap();
        match result {
            Ok(()) => winners += 1,
            Err(error) => {
                assert_eq!(
                    error.downcast_ref::<FileAtlasError>(),
                    Some(&FileAtlasError::KeyAlreadyPresent)
                );
                loser_paths.push(path);
            }
        }
    }

    assert_eq!(winners, 1);
    assert_eq!(atlas.len(), 1);
    for path in loser_paths {
        if path.exists() {
            assert!(can_lock(&path), "losing file is still locked");
        }
    }
}

/// Key whose `Hash` panics for the value 13.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PanicKey(u32);

impl Hash for PanicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        assert_ne!(self.0, 13, "intentional panic in Hash");
        self.0.hash(state);
    }
}

// T-018
#[test]
fn atlas_survives_a_panicking_key_hash() {
    let directory = tempdir().unwrap();
    let atlas = FileAtlas::new();
    atlas
        .open_or_create(PanicKey(1), path_in(&directory, "1"), ROW_SIZE, 0)
        .unwrap();

    // `remove` hashes under the map write lock, so this panic poisons it.
    let result = panic::catch_unwind(AssertUnwindSafe(|| atlas.remove(&PanicKey(13))));
    assert!(result.is_err());

    atlas
        .open_or_create(PanicKey(2), path_in(&directory, "2"), ROW_SIZE, 0)
        .unwrap();
    assert!(atlas.get(&PanicKey(1)).is_some());
    assert_eq!(atlas.len(), 2);
    assert!(atlas.remove(&PanicKey(1)).is_ok());
    assert_eq!(atlas.len(), 1);
}

// T-019
#[test]
fn atlas_is_send_sync_and_adds_no_files() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FileAtlas<String>>();

    let directory = tempdir().unwrap();
    let path = path_in(&directory, "key");
    let atlas = atlas_with_rows(&directory, "key", 3);
    drop(atlas.remove("key").unwrap());

    let metadata = FileMetadata::open_or_create(&path, ROW_SIZE, 0).unwrap();
    assert_eq!(metadata.element_count().unwrap(), 3);
    assert_eq!(read_value(&metadata, 1), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

fn atlas_with_keys(directory: &TempDir, keys: &[&str]) -> FileAtlas<String> {
    let atlas = FileAtlas::new();
    for key in keys {
        atlas
            .open_or_create((*key).to_owned(), path_in(directory, key), ROW_SIZE, 0)
            .unwrap();
    }
    atlas
}

// T-020
#[test]
fn iter_returns_a_snapshot_of_entries() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_keys(&directory, &["a", "b", "c"]);

    let mut snapshot = atlas.iter();
    assert_eq!(snapshot.len(), 3);
    let (first_key, first_handle) = snapshot.next().unwrap();
    assert_eq!(snapshot.len(), 2);

    atlas
        .open_or_create("d".to_owned(), path_in(&directory, "d"), ROW_SIZE, 0)
        .unwrap();
    assert!(atlas.remove("d").is_ok());

    let mut seen = vec![(first_key, first_handle)];
    seen.extend(snapshot.by_ref());
    assert_eq!(snapshot.len(), 0);
    assert!(snapshot.next().is_none());
    assert!(snapshot.next().is_none());

    let keys: HashSet<_> = seen.iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(keys, HashSet::from(["a", "b", "c"].map(str::to_owned)));
    for (key, handle) in &seen {
        assert!(Arc::ptr_eq(handle, &atlas.get(key.as_str()).unwrap()));
    }
}

// T-021
#[test]
fn borrowed_iteration_allows_atlas_calls_and_blocks_remove() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_keys(&directory, &["a", "b"]);

    for (key, handle) in &atlas {
        // The other key may already have the row appended by an earlier pass.
        assert!(handle.read().unwrap().element_count().unwrap() <= 1);
        let new_key = format!("new-{key}");
        atlas
            .open_or_create(new_key.clone(), path_in(&directory, &new_key), ROW_SIZE, 0)
            .unwrap();
        let other = if key == "a" { "b" } else { "a" };
        atlas
            .with_write(other, |file| file.append_row(&row(1)))
            .unwrap();
        assert_eq!(
            atlas_error(atlas.remove(key.as_str())),
            FileAtlasError::InUse
        );
    }

    assert_eq!(atlas.len(), 4);
    assert!(atlas.remove("a").is_ok());
    assert!(atlas.remove("b").is_ok());
}

// T-022
#[test]
fn owned_iteration_releases_files_as_handles_drop() {
    let directory = tempdir().unwrap();
    let atlas = atlas_with_keys(&directory, &["kept", "dropped"]);

    let entries = atlas.into_iter();
    assert_eq!(entries.len(), 2);
    let mut kept = None;
    let mut yielded = 0;
    for (key, handle) in entries {
        yielded += 1;
        if key == "kept" {
            kept = Some(handle);
        }
    }

    assert_eq!(yielded, 2);
    assert!(can_lock(&path_in(&directory, "dropped")));
    assert!(!can_lock(&path_in(&directory, "kept")));
    drop(kept);
    assert!(can_lock(&path_in(&directory, "kept")));
}

// T-023
#[test]
fn atlas_iterators_have_the_expected_traits() {
    fn assert_traits<I>()
    where
        I: Iterator + ExactSizeIterator + FusedIterator + Send + Sync,
    {
    }
    assert_traits::<FileAtlasIter<String>>();
    assert_traits::<FileAtlasIntoIter<String>>();

    // The probe handle in `can_lock` must not keep the file busy.
    let directory = tempdir().unwrap();
    let path = path_in(&directory, "probe");
    File::create(&path).unwrap();
    assert!(can_lock(&path));
    assert!(can_lock(&path));
}
