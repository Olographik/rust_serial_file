use std::{
    borrow::{Borrow, BorrowMut},
    collections::HashSet,
    fs,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{Arc, Barrier, mpsc},
    thread,
    time::Duration,
};

use rust_serial_file::adapter::{
    abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter,
};
use rust_serial_file::file::{
    buffered_reader::BufferedReader,
    buffered_writer::BufferedWriter,
    file_lock::{FileLock, FileReadGuard, FileSnapshot, FileWriteGuard},
    file_metadata::FileMetadata,
    struct_file::StructFile,
};
use rust_serial_file::serialized_struct;
use tempfile::{TempDir, tempdir};

const ROW_SIZE: u64 = 4;
const POISONER: &str = "poisoner";

fn row(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

fn path_in(directory: &TempDir) -> PathBuf {
    directory.path().join("records.bin")
}

/// Creates a lock over a file holding `count` rows with values `0..count`.
fn lock_with_rows(count: u32) -> (TempDir, FileLock) {
    let directory = tempdir().unwrap();
    let mut metadata = FileMetadata::open_or_create(path_in(&directory), ROW_SIZE, 0).unwrap();
    for value in 0..count {
        metadata.append_row(&row(value)).unwrap();
    }
    (directory, FileLock::new(metadata))
}

fn read_value(metadata: &FileMetadata, index: u64) -> u32 {
    let mut buffer = [0u8; 4];
    metadata.read_row(index, &mut buffer).unwrap();
    u32::from_le_bytes(buffer)
}

/// Runs `action` with the write guard in a thread named `POISONER`, then
/// panics so the lock is poisoned.
fn poison_with(lock: &Arc<FileLock>, action: impl FnOnce(&mut FileMetadata) + Send + 'static) {
    let lock = Arc::clone(lock);
    let result = thread::Builder::new()
        .name(POISONER.to_owned())
        .spawn(move || {
            let mut guard = lock.write().unwrap();
            action(&mut guard);
            panic!("intentional panic while holding the write guard");
        })
        .unwrap()
        .join();
    assert!(result.is_err());
}

fn assert_poisoned<T>(result: anyhow::Result<T>) {
    match result {
        Ok(_) => panic!("expected a poisoning error"),
        Err(error) => assert!(error.to_string().contains("poisoned"), "{error}"),
    }
}

fn directory_entry_count(path: &Path) -> usize {
    fs::read_dir(path).unwrap().count()
}

// T-001
#[test]
fn new_and_into_inner_keep_metadata() {
    let directory = tempdir().unwrap();
    let mut metadata = FileMetadata::open_or_create(path_in(&directory), ROW_SIZE, 2).unwrap();
    metadata.update_raw_metadata(&[7, 8]).unwrap();
    metadata.append_row(&row(1)).unwrap();

    let metadata = FileLock::new(metadata).into_inner().unwrap();
    assert_eq!(metadata.element_size(), ROW_SIZE);
    assert_eq!(metadata.data_offset(), 2);
    assert_eq!(metadata.element_count().unwrap(), 1);

    let metadata = FileLock::from(metadata).into_inner().unwrap();
    assert_eq!(metadata.element_size(), ROW_SIZE);
    assert_eq!(metadata.data_offset(), 2);
    assert_eq!(metadata.element_count().unwrap(), 1);
}

// T-002
#[test]
fn open_or_create_forwards_metadata_errors() {
    let directory = tempdir().unwrap();
    let lock = FileLock::open_or_create(path_in(&directory), ROW_SIZE, 0).unwrap();
    assert_eq!(lock.read().unwrap().element_count().unwrap(), 0);

    let error = FileLock::open_or_create(directory.path().join("zero.bin"), 0, 0)
        .err()
        .expect("element_size 0 must fail");
    assert!(error.to_string().contains("element_size"), "{error}");
}

// T-003
#[test]
fn get_mut_gives_unlocked_mutable_access() {
    let (_directory, mut lock) = lock_with_rows(0);
    lock.get_mut().unwrap().append_row(&row(5)).unwrap();
    assert_eq!(lock.get_mut().unwrap().element_count().unwrap(), 1);
}

// T-004
#[test]
fn file_lock_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FileLock>();
}

// T-005
#[test]
fn locking_performs_no_file_io() {
    let (directory, lock) = lock_with_rows(3);
    let file_len = fs::metadata(path_in(&directory)).unwrap().len();
    let entries = directory_entry_count(directory.path());

    for _ in 0..3 {
        drop(lock.read().unwrap());
        drop(lock.write().unwrap());
        drop(lock.try_read().unwrap());
        drop(lock.try_write().unwrap());
    }

    assert_eq!(fs::metadata(path_in(&directory)).unwrap().len(), file_len);
    assert_eq!(lock.read().unwrap().element_count().unwrap(), 3);
    assert_eq!(directory_entry_count(directory.path()), entries);
}

// T-006
#[test]
fn multiple_read_guards_coexist() {
    let (_directory, lock) = lock_with_rows(2);
    let first = lock.read().unwrap();
    let second = lock.read().unwrap();
    assert_eq!(read_value(&first, 1), 1);
    assert_eq!(read_value(&second, 1), 1);
}

// T-007
#[test]
fn write_guard_allows_mutation() {
    let (_directory, lock) = lock_with_rows(1);
    {
        let mut guard = lock.write().unwrap();
        guard.write_row(0, &row(10)).unwrap();
        guard.append_row(&row(11)).unwrap();
        guard.append_row(&row(12)).unwrap();
        assert_eq!(guard.element_count().unwrap(), 3);
    }
    assert_eq!(lock.read().unwrap().element_count().unwrap(), 3);
}

// T-008
#[test]
fn writes_visible_after_write_guard_drop() {
    let (_directory, lock) = lock_with_rows(2);
    {
        let mut guard = lock.write().unwrap();
        guard.write_row(1, &row(42)).unwrap();
        guard.insert_row(0, &row(7)).unwrap();
    }
    let guard = lock.read().unwrap();
    assert_eq!(guard.element_count().unwrap(), 3);
    assert_eq!(read_value(&guard, 0), 7);
    assert_eq!(read_value(&guard, 1), 0);
    assert_eq!(read_value(&guard, 2), 42);
}

// T-009
#[test]
fn read_guard_supports_row_iteration() {
    let (_directory, lock) = lock_with_rows(4);
    let guard = lock.read().unwrap();
    let values: Vec<u32> = guard
        .rows()
        .unwrap()
        .map(|bytes| u32::from_le_bytes(bytes.unwrap().try_into().unwrap()))
        .collect();
    assert_eq!(values, vec![0, 1, 2, 3]);
}

// T-010
#[test]
fn read_guard_released_on_unwind() {
    let (_directory, lock) = lock_with_rows(1);
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let _guard = lock.read().unwrap();
        panic!("intentional panic while holding a read guard");
    }));
    assert!(result.is_err());

    // Read guards do not poison: the write guard is available.
    assert!(lock.try_write().unwrap().is_some());
    assert_eq!(lock.recover().unwrap(), None);
}

// T-011
#[test]
fn try_locks_while_reading() {
    let (_directory, lock) = lock_with_rows(1);
    let _reader = lock.read().unwrap();
    assert!(lock.try_read().unwrap().is_some());
    assert!(lock.try_write().unwrap().is_none());
}

// T-012
#[test]
fn try_locks_while_writing() {
    let (_directory, lock) = lock_with_rows(1);
    let writer = lock.write().unwrap();
    assert!(lock.try_read().unwrap().is_none());
    assert!(lock.try_write().unwrap().is_none());
    drop(writer);
    assert!(lock.try_read().unwrap().is_some());
    assert!(lock.try_write().unwrap().is_some());
}

// T-013
#[test]
fn try_write_after_last_read_guard_drop() {
    let (_directory, lock) = lock_with_rows(1);
    let first = lock.read().unwrap();
    let second = lock.read().unwrap();
    drop(first);
    assert!(lock.try_write().unwrap().is_none());
    drop(second);
    assert!(lock.try_write().unwrap().is_some());
}

// T-014
#[test]
fn poisoned_lock_returns_errors() {
    let (_directory, lock) = lock_with_rows(1);
    let lock = Arc::new(lock);
    poison_with(&lock, |_| {});

    assert_poisoned(lock.read());
    assert_poisoned(lock.write());
    assert_poisoned(lock.try_read());
    assert_poisoned(lock.try_write());
    let mut lock = Arc::into_inner(lock).unwrap();
    assert_poisoned(lock.get_mut());
    assert_poisoned(lock.into_inner());
}

// T-015
#[test]
fn concurrent_readers_across_threads() {
    const READERS: usize = 4;
    let (_directory, lock) = lock_with_rows(1);
    let lock = Arc::new(lock);
    let barrier = Arc::new(Barrier::new(READERS));

    let handles: Vec<_> = (0..READERS)
        .map(|_| {
            let lock = Arc::clone(&lock);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let guard = lock.read().unwrap();
                // Every reader waits here while holding its guard: this only
                // completes if all read guards are held at the same time.
                barrier.wait();
                read_value(&guard, 0)
            })
        })
        .collect();

    for handle in handles {
        assert_eq!(handle.join().unwrap(), 0);
    }
}

// T-016
#[test]
fn writer_blocks_readers_until_released() {
    let (_directory, lock) = lock_with_rows(1);
    let lock = Arc::new(lock);
    let writer = lock.write().unwrap();

    let (sender, receiver) = mpsc::channel();
    let reader_lock = Arc::clone(&lock);
    let reader = thread::spawn(move || {
        let guard = reader_lock.read().unwrap();
        sender.send(read_value(&guard, 0)).unwrap();
    });

    assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
    drop(writer);
    assert_eq!(receiver.recv_timeout(Duration::from_secs(10)).unwrap(), 0);
    reader.join().unwrap();
}

// T-017
#[test]
fn concurrent_writers_are_serialized() {
    const WRITERS: u32 = 4;
    const ROWS_PER_WRITER: u32 = 50;
    let (_directory, lock) = lock_with_rows(0);
    let lock = Arc::new(lock);

    let handles: Vec<_> = (0..WRITERS)
        .map(|writer| {
            let lock = Arc::clone(&lock);
            thread::spawn(move || {
                for sequence in 0..ROWS_PER_WRITER {
                    let mut guard = lock.write().unwrap();
                    guard
                        .append_row(&row(writer * ROWS_PER_WRITER + sequence))
                        .unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let guard = lock.read().unwrap();
    let total = u64::from(WRITERS * ROWS_PER_WRITER);
    assert_eq!(guard.element_count().unwrap(), total);
    let values: HashSet<u32> = (0..total).map(|index| read_value(&guard, index)).collect();
    assert_eq!(values, (0..WRITERS * ROWS_PER_WRITER).collect());
    assert_eq!(lock.generation(), total);
}

// T-018
#[test]
fn file_written_through_lock_reopens_as_metadata() {
    let directory = tempdir().unwrap();
    let lock = FileLock::open_or_create(path_in(&directory), ROW_SIZE, 0).unwrap();
    {
        let mut guard = lock.write().unwrap();
        guard.update_raw_metadata(&[1, 2, 3]).unwrap();
        guard.append_row(&row(100)).unwrap();
        guard.append_row(&row(200)).unwrap();
    }
    drop(lock.into_inner().unwrap());

    let reopened = FileMetadata::open_or_create(path_in(&directory), ROW_SIZE, 3).unwrap();
    let mut header = [0u8; 3];
    reopened.get_raw_metadata(&mut header).unwrap();
    assert_eq!(header, [1, 2, 3]);
    assert_eq!(reopened.element_count().unwrap(), 2);
    assert_eq!(read_value(&reopened, 0), 100);
    assert_eq!(read_value(&reopened, 1), 200);
}

// T-019
#[test]
fn recover_reports_snapshot_of_interrupted_write() {
    let (_directory, lock) = lock_with_rows(3);
    let lock = Arc::new(lock);
    poison_with(&lock, |metadata| metadata.append_row(&row(3)).unwrap());

    let report = lock.recover().unwrap().expect("poisoned lock has a report");
    assert_eq!(
        report.before,
        FileSnapshot {
            file_len: 3 * ROW_SIZE,
            element_count: 3,
            data_offset: 0,
        }
    );
    assert_eq!(
        report.after,
        Some(FileSnapshot {
            file_len: 4 * ROW_SIZE,
            element_count: 4,
            data_offset: 0,
        })
    );
    assert_eq!(report.thread_name.as_deref(), Some(POISONER));
}

// T-020
#[test]
fn recover_clears_poison() {
    let (_directory, lock) = lock_with_rows(1);
    let lock = Arc::new(lock);
    poison_with(&lock, |_| {});

    assert!(lock.recover().unwrap().is_some());
    assert_eq!(read_value(&lock.read().unwrap(), 0), 0);
    lock.write().unwrap().append_row(&row(1)).unwrap();
    assert_eq!(lock.recover().unwrap(), None);

    let metadata = Arc::into_inner(lock).unwrap().into_inner().unwrap();
    assert_eq!(metadata.element_count().unwrap(), 2);
}

// T-021
#[test]
fn recover_on_healthy_lock_is_noop() {
    let (directory, lock) = lock_with_rows(2);
    let lock = Arc::new(lock);
    let file_len = fs::metadata(path_in(&directory)).unwrap().len();

    assert_eq!(lock.recover().unwrap(), None);

    // `recover` blocks like `write` while a reader holds the lock.
    let (ready_sender, ready_receiver) = mpsc::channel();
    let reader_lock = Arc::clone(&lock);
    let reader = thread::spawn(move || {
        let _guard = reader_lock.read().unwrap();
        ready_sender.send(()).unwrap();
        thread::sleep(Duration::from_millis(50));
    });
    ready_receiver.recv().unwrap();
    assert_eq!(lock.recover().unwrap(), None);
    reader.join().unwrap();

    assert_eq!(lock.generation(), 0);
    assert_eq!(fs::metadata(path_in(&directory)).unwrap().len(), file_len);
    assert_eq!(lock.read().unwrap().element_count().unwrap(), 2);
}

// T-022
#[test]
fn generation_counts_clean_write_releases() {
    let (_directory, mut lock) = lock_with_rows(1);
    assert_eq!(lock.generation(), 0);

    drop(lock.write().unwrap());
    drop(lock.try_write().unwrap());
    assert_eq!(lock.generation(), 2);

    drop(lock.read().unwrap());
    drop(lock.read().unwrap());
    lock.get_mut().unwrap().append_row(&row(1)).unwrap();
    assert_eq!(lock.generation(), 2);

    let lock = Arc::new(lock);
    poison_with(&lock, |_| {});
    assert_eq!(lock.generation(), 2);
    assert!(lock.recover().unwrap().is_some());
    assert_eq!(lock.generation(), 2);
}

// T-023
#[test]
fn report_does_not_detect_in_place_overwrite() {
    let (_directory, lock) = lock_with_rows(2);
    let lock = Arc::new(lock);
    poison_with(&lock, |metadata| metadata.write_row(1, &row(99)).unwrap());

    let report = lock.recover().unwrap().expect("poisoned lock has a report");
    assert_eq!(Some(report.before.clone()), report.after);
    // The row did change, which the report cannot show.
    assert_eq!(read_value(&lock.read().unwrap(), 1), 99);
}

// T-024
#[test]
fn buffered_reader_borrows_through_read_guard() {
    let (_directory, lock) = lock_with_rows(10);
    let guard = lock.read().unwrap();
    let mut reader = BufferedReader::with_capacity(&guard, 3).unwrap();

    assert_eq!(reader.len(), 10);
    assert_eq!(reader.read_row(7).unwrap(), row(7));
    let values: Vec<u32> = reader
        .rows()
        .map(|bytes| u32::from_le_bytes(bytes.unwrap().try_into().unwrap()))
        .collect();
    assert_eq!(values, (0..10).collect::<Vec<_>>());

    // A second reader can borrow through another read guard at the same time.
    let other_guard = lock.read().unwrap();
    let mut other = BufferedReader::new(&other_guard).unwrap();
    assert_eq!(other.read_row(2).unwrap(), row(2));
}

// T-025
#[test]
fn buffered_writer_borrows_through_write_guard() {
    let (_directory, lock) = lock_with_rows(2);
    {
        let mut guard = lock.write().unwrap();
        let mut writer = BufferedWriter::with_capacity(&mut guard, 4).unwrap();
        writer.write_row(0, &row(50)).unwrap();
        for value in 2..7 {
            writer.write_row(u64::from(value), &row(value)).unwrap();
        }
        assert_eq!(writer.len(), 7);
        writer.flush().unwrap();
        drop(writer);
        // The guard is usable again once the writer's borrow ends.
        assert_eq!(guard.element_count().unwrap(), 7);
    }

    let guard = lock.read().unwrap();
    assert_eq!(read_value(&guard, 0), 50);
    assert_eq!(read_value(&guard, 1), 1);
    assert_eq!(read_value(&guard, 6), 6);
    assert_eq!(lock.generation(), 1);
}

// T-026
#[test]
fn buffered_writer_drop_flushes_before_write_lock_release() {
    let (_directory, lock) = lock_with_rows(0);
    {
        let mut guard = lock.write().unwrap();
        let mut writer = BufferedWriter::with_capacity(&mut guard, 16).unwrap();
        for value in 0..5 {
            writer.write_row(u64::from(value), &row(value)).unwrap();
        }
        // No explicit flush: the writer is dropped first (it borrows the
        // guard), so its pending rows reach the file before the lock is
        // released.
    }

    let guard = lock.read().unwrap();
    assert_eq!(guard.element_count().unwrap(), 5);
    let mut reader = BufferedReader::new(&guard).unwrap();
    assert_eq!(reader.read_row(4).unwrap(), row(4));
}

// T-027
#[test]
fn buffered_readers_on_concurrent_threads() {
    const READERS: usize = 4;
    let (_directory, lock) = lock_with_rows(20);
    let lock = Arc::new(lock);
    let barrier = Arc::new(Barrier::new(READERS));

    let handles: Vec<_> = (0..READERS)
        .map(|_| {
            let lock = Arc::clone(&lock);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let guard = lock.read().unwrap();
                let mut reader = BufferedReader::with_capacity(&guard, 5).unwrap();
                barrier.wait();
                let mut sum = 0u64;
                for index in 0..reader.len() {
                    let bytes: [u8; 4] = reader.read_row(index).unwrap().try_into().unwrap();
                    sum += u64::from(u32::from_le_bytes(bytes));
                }
                sum
            })
        })
        .collect();

    for handle in handles {
        assert_eq!(handle.join().unwrap(), (0..20).sum::<u64>());
    }
}

// T-028
#[test]
fn buffered_writer_blocks_buffered_readers_until_released() {
    let (_directory, lock) = lock_with_rows(1);
    let lock = Arc::new(lock);
    let (written_sender, written_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel::<()>();

    let writer_lock = Arc::clone(&lock);
    let writer_thread = thread::spawn(move || {
        let mut guard = writer_lock.write().unwrap();
        let mut writer = BufferedWriter::with_capacity(&mut guard, 8).unwrap();
        writer.write_row(1, &row(11)).unwrap();
        written_sender.send(()).unwrap();
        release_receiver.recv().unwrap();
    });

    written_receiver.recv().unwrap();
    assert!(lock.try_read().unwrap().is_none());

    let (seen_sender, seen_receiver) = mpsc::channel();
    let reader_lock = Arc::clone(&lock);
    let reader_thread = thread::spawn(move || {
        let guard = reader_lock.read().unwrap();
        let mut reader = BufferedReader::new(&guard).unwrap();
        let bytes: [u8; 4] = reader.read_row(1).unwrap().try_into().unwrap();
        seen_sender.send(u32::from_le_bytes(bytes)).unwrap();
    });

    assert!(
        seen_receiver
            .recv_timeout(Duration::from_millis(200))
            .is_err()
    );
    release_sender.send(()).unwrap();
    // The writer's pending row was flushed before the write lock was released.
    assert_eq!(
        seen_receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
        11
    );
    writer_thread.join().unwrap();
    reader_thread.join().unwrap();
}

// T-029
#[test]
fn panic_with_buffered_writer_reports_flushed_rows() {
    let (_directory, lock) = lock_with_rows(2);
    let lock = Arc::new(lock);
    poison_with(&lock, |metadata| {
        let mut writer = BufferedWriter::with_capacity(metadata, 8).unwrap();
        writer.write_row(2, &row(2)).unwrap();
        writer.write_row(3, &row(3)).unwrap();
        // `poison_with` panics after this closure; the writer is dropped
        // (and flushed) here, before the guard records its `after` snapshot.
    });

    let report = lock.recover().unwrap().expect("poisoned lock has a report");
    assert_eq!(report.before.element_count, 2);
    assert_eq!(report.after.map(|after| after.element_count), Some(4));
}

// T-030
#[test]
fn panic_while_buffered_writer_alive_flushes_during_unwind() {
    let (_directory, lock) = lock_with_rows(0);
    let lock = Arc::new(lock);
    let panicking_lock = Arc::clone(&lock);
    let result = thread::spawn(move || {
        let mut guard = panicking_lock.write().unwrap();
        let mut writer = BufferedWriter::with_capacity(&mut guard, 8).unwrap();
        writer.write_row(0, &row(9)).unwrap();
        panic!("intentional panic while a buffered writer has pending rows");
    })
    .join();
    assert!(result.is_err());

    // Unwinding drops the writer (flushing its row) before the guard, so the
    // report sees the flushed row.
    let report = lock.recover().unwrap().expect("poisoned lock has a report");
    assert_eq!(report.before.element_count, 0);
    assert_eq!(report.after.map(|after| after.element_count), Some(1));
    assert_eq!(read_value(&lock.read().unwrap(), 0), 9);
}

/// Typed record matching the 4-byte little-endian rows used in this file.
#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq, PartialOrd)]
struct Value {
    value: u32,
}

fn value(value: u32) -> Value {
    Value { value }
}

fn read_values<V: SerialDataReader<Value>>(view: &V) -> Vec<u32> {
    view.serial_iter()
        .unwrap()
        .map(|record| record.unwrap().value)
        .collect()
}

// T-031
#[test]
fn guard_borrow_matches_deref() {
    let (_directory, lock) = lock_with_rows(2);

    {
        let guard = lock.read().unwrap();
        let borrowed: &FileMetadata = guard.borrow();
        assert!(std::ptr::eq(borrowed, &*guard));
    }
    {
        let mut guard = lock.write().unwrap();
        let borrowed: &FileMetadata = guard.borrow();
        assert!(std::ptr::eq(borrowed, &*guard));
        let deref_ptr: *const FileMetadata = &*guard;
        let borrowed_mut: &mut FileMetadata = guard.borrow_mut();
        assert!(std::ptr::eq(borrowed_mut, deref_ptr));
        borrowed_mut.append_row(&row(7)).unwrap();
    }

    let guard = lock.read().unwrap();
    assert_eq!(guard.element_count().unwrap(), 3);
    assert_eq!(read_value(&guard, 2), 7);
    assert_eq!(lock.generation(), 1);
}

// T-032
#[test]
fn struct_file_views_hold_read_guards() {
    let (_directory, lock) = lock_with_rows(3);

    {
        let first = StructFile::<Value, _>::new(lock.read().unwrap()).unwrap();
        let second = StructFile::<Value, FileReadGuard<'_>>::new(lock.read().unwrap()).unwrap();
        assert_eq!(read_values(&first), [0, 1, 2]);
        assert_eq!(second.serial_get(1).unwrap(), value(1));
        assert!(lock.try_write().unwrap().is_none());

        drop(first);
        assert!(lock.try_write().unwrap().is_none());
    }

    assert!(lock.try_write().unwrap().is_some());
}

// T-033
#[test]
fn struct_file_view_holds_write_guard_until_dropped() {
    let (_directory, lock) = lock_with_rows(2);

    let mut writer = StructFile::<Value, FileWriteGuard<'_>>::new(lock.write().unwrap()).unwrap();
    writer.serial_write(value(5)).unwrap();
    writer.serial_insert(1, value(9)).unwrap();
    assert!(lock.try_read().unwrap().is_none());

    let guard = writer.into_inner();
    assert_eq!(guard.element_count().unwrap(), 4);
    assert_eq!(lock.generation(), 0);
    drop(guard);
    assert_eq!(lock.generation(), 1);

    let reader = StructFile::<Value, _>::new(lock.read().unwrap()).unwrap();
    assert_eq!(read_values(&reader), [0, 9, 1, 5]);
}

// T-034
#[test]
fn panic_with_struct_file_write_view_poisons_and_reports() {
    let (_directory, lock) = lock_with_rows(3);
    let lock = Arc::new(lock);

    let poisoner = Arc::clone(&lock);
    let result = thread::Builder::new()
        .name(POISONER.to_owned())
        .spawn(move || {
            let mut writer = StructFile::<Value, _>::new(poisoner.write().unwrap()).unwrap();
            writer.serial_write(value(10)).unwrap();
            writer.serial_write(value(11)).unwrap();
            panic!("intentional panic while a write view holds the guard");
        })
        .unwrap()
        .join();
    assert!(result.is_err());

    assert_poisoned(lock.read());
    let report = lock.recover().unwrap().unwrap();
    assert_eq!(report.before.element_count, 3);
    assert_eq!(report.after.unwrap().element_count, 5);
    assert_eq!(report.thread_name.as_deref(), Some(POISONER));

    let reader = StructFile::<Value, _>::new(lock.read().unwrap()).unwrap();
    assert_eq!(read_values(&reader), [0, 1, 2, 10, 11]);
}
