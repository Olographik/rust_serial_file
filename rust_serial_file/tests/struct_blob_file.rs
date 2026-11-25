use std::{fs, path::PathBuf};

use rust_serial_file::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    file::{
        blob_file::{BlobFields, BlobRef, compact_path},
        struct_blob_file::StructBlobFile,
    },
    serialized_struct,
};
use serialized_struct_derive::SerializedStruct;
use tempfile::{TempDir, tempdir};

/// A record with two variable-length fields.
#[derive(SerializedStruct, Debug, Clone, Copy, PartialEq)]
struct Sample {
    time: u64,
    name: BlobRef,
    note: BlobRef,
}

impl BlobFields for Sample {
    fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef)) {
        f(&mut self.name);
        f(&mut self.note);
    }
}

fn paths(directory: &TempDir) -> (PathBuf, PathBuf) {
    (
        directory.path().join("samples.bin"),
        directory.path().join("samples.blob"),
    )
}

fn open(directory: &TempDir, data_offset: u64) -> StructBlobFile<Sample> {
    let (rows, blob) = paths(directory);
    StructBlobFile::open_or_create(rows, blob, data_offset).unwrap()
}

/// Appends a row with the given name and note.
fn push(file: &mut StructBlobFile<Sample>, time: u64, name: &str, note: &str) {
    let (mut rows, blob) = file.parts_mut().unwrap();
    let name = blob.write_str(name).unwrap();
    let note = blob.write_str(note).unwrap();
    rows.serial_write(Sample { time, name, note }).unwrap();
}

/// Every row as (time, name, note) strings.
fn contents(file: &StructBlobFile<Sample>) -> Vec<(u64, String, String)> {
    let (rows, blob) = file.parts().unwrap();
    rows.serial_iter()
        .unwrap()
        .map(|row| {
            let row = row.unwrap();
            (
                row.time,
                blob.read_string(row.name).unwrap(),
                blob.read_string(row.note).unwrap(),
            )
        })
        .collect()
}

#[test]
fn compacts_and_updates_rows() {
    let directory = tempdir().unwrap();
    let mut file = open(&directory, 0);
    for time in 0..6 {
        push(&mut file, time, &format!("sensor-{time}"), "");
    }
    // Grow some values so they move, leaving holes behind.
    {
        let (mut rows, blob) = file.parts_mut().unwrap();
        for index in [1_usize, 3] {
            let mut row = rows.serial_get(index).unwrap();
            row.name = blob
                .update(
                    row.name,
                    format!("renamed sensor number {index}").as_bytes(),
                )
                .unwrap();
            rows.serial_write_at(index, row).unwrap();
        }
        // A value no row references: a leak, as after a crash.
        blob.write(b"leaked value").unwrap();
    }
    let expected = contents(&file);
    let before = file.blob().stats();
    assert!(before.free_blocks > 0);

    let stats = file.compact().unwrap();
    assert_eq!(contents(&file), expected);
    assert_eq!(stats, file.blob().stats());
    assert_eq!(stats.free_blocks, 0);
    // Only the 6 names are left (the notes are empty); the leak is gone.
    assert_eq!(stats.live_blocks, 6);
    assert_eq!(stats.file_len, 16 + 6 * 8 + stats.live_bytes);
    assert!(stats.file_len < before.file_len);

    // Values are laid out in row order, without holes.
    let (rows, _) = file.parts().unwrap();
    let mut next_offset = 16 + 8;
    for row in rows.serial_iter().unwrap() {
        let row = row.unwrap();
        for reference in [row.name, row.note] {
            if !reference.is_empty() {
                assert_eq!(reference.offset, next_offset);
                next_offset += u64::from(reference.len).div_ceil(8) * 8 + 8;
            }
        }
    }

    // No temporary file is left, and the files reopen as compacted.
    let (rows_path, blob_path) = paths(&directory);
    assert!(!compact_path(&rows_path).exists());
    assert!(!compact_path(&blob_path).exists());
    drop(file);
    let reopened = open(&directory, 0);
    assert_eq!(contents(&reopened), expected);
    assert_eq!(reopened.blob().stats(), stats);
}

#[test]
fn compaction_keeps_the_metadata_prefix_and_handles_still_work() {
    let directory = tempdir().unwrap();
    let mut file = open(&directory, 4);
    {
        let (mut rows, _) = file.parts_mut().unwrap();
        rows.serial_write(Sample {
            time: 1,
            name: BlobRef::EMPTY,
            note: BlobRef::EMPTY,
        })
        .unwrap();
    }
    let (metadata, blob) = file.into_parts();
    let mut metadata = metadata;
    metadata.update_raw_metadata(b"HEAD").unwrap();
    drop((metadata, blob));

    let mut file = open(&directory, 4);
    push(&mut file, 2, "two", "2");
    file.compact().unwrap();
    // Writing after compaction goes to the new files.
    push(&mut file, 3, "three", "3");
    file.sync_data().unwrap();
    drop(file);

    let (rows_path, _) = paths(&directory);
    assert_eq!(&fs::read(&rows_path).unwrap()[..4], b"HEAD");
    let reopened = open(&directory, 4);
    assert_eq!(
        contents(&reopened),
        [
            (1, String::new(), String::new()),
            (2, "two".to_owned(), "2".to_owned()),
            (3, "three".to_owned(), "3".to_owned()),
        ]
    );
}

#[test]
fn failed_compaction_leaves_files_unchanged() {
    let directory = tempdir().unwrap();
    let mut file = open(&directory, 0);
    push(&mut file, 1, "one", "");
    {
        // A row pointing to a value that does not exist.
        let (mut rows, _) = file.parts_mut().unwrap();
        rows.serial_write(Sample {
            time: 2,
            name: BlobRef::new(4096, 3),
            note: BlobRef::EMPTY,
        })
        .unwrap();
    }
    let (rows_path, blob_path) = paths(&directory);
    let rows_before = fs::read(&rows_path).unwrap();
    let blob_before = fs::read(&blob_path).unwrap();

    let error = format!("{:#}", file.compact().unwrap_err());
    assert!(error.contains("original files are unchanged"), "{error}");
    assert!(error.contains("invalid blob reference"), "{error}");
    assert_eq!(fs::read(&rows_path).unwrap(), rows_before);
    assert_eq!(fs::read(&blob_path).unwrap(), blob_before);
    assert!(!compact_path(&rows_path).exists());
    assert!(!compact_path(&blob_path).exists());
    // The handles still work.
    assert_eq!(file.parts().unwrap().0.serial_len(), 2);
}

#[test]
fn refuses_leftover_compact_file() {
    let directory = tempdir().unwrap();
    let (rows_path, blob_path) = paths(&directory);
    let mut file = open(&directory, 0);
    push(&mut file, 1, "one", "");

    for leftover in [compact_path(&rows_path), compact_path(&blob_path)] {
        fs::write(&leftover, b"").unwrap();
        let message = match StructBlobFile::<Sample>::open_or_create(&rows_path, &blob_path, 0) {
            Ok(_) => panic!("opening must fail while {} exists", leftover.display()),
            Err(error) => format!("{error:#}"),
        };
        assert!(message.contains("interrupted compaction"), "{message}");
        // Compacting the open pair is refused too, without touching it.
        let message = format!("{:#}", file.compact().unwrap_err());
        assert!(message.contains(".compact"), "{message}");
        fs::remove_file(&leftover).unwrap();
    }
    assert_eq!(contents(&file), [(1, "one".to_owned(), String::new())]);
}

#[test]
fn sync_and_reopen() {
    let directory = tempdir().unwrap();
    let mut file = open(&directory, 0);
    push(&mut file, 1, "one", "first");
    file.sync_data().unwrap();
    push(&mut file, 2, "two", "second");
    file.sync_all().unwrap();
    assert_eq!(file.rows().unwrap().serial_len(), 2);
    drop(file);

    let reopened = open(&directory, 0);
    assert_eq!(
        contents(&reopened),
        [
            (1, "one".to_owned(), "first".to_owned()),
            (2, "two".to_owned(), "second".to_owned()),
        ]
    );
}
