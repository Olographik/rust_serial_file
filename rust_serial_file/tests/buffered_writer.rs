use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

use rust_serial_file::file::{buffered_writer::BufferedWriter, file_metadata::FileMetadata};
use tempfile::tempdir;

fn open_metadata(
    bytes: &[u8],
    element_size: u64,
) -> (tempfile::TempDir, std::path::PathBuf, FileMetadata) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(bytes).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let metadata = FileMetadata::new(file, element_size, 0).unwrap();
    (directory, path, metadata)
}

fn read_rows(metadata: &mut FileMetadata) -> Vec<u8> {
    let mut bytes = Vec::new();
    for row in metadata.rows().unwrap() {
        bytes.extend_from_slice(&row.unwrap());
    }
    bytes
}

fn read_file(path: &std::path::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn replace_file(path: &std::path::Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn constructs_from_borrowed_file_metadata() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);

    {
        let writer = BufferedWriter::new(&mut metadata).unwrap();
        assert_eq!(writer.len(), 2);
    }

    assert_eq!(metadata.element_count().unwrap(), 2);
}

#[test]
fn reports_empty_logical_length() {
    let (_directory, _path, mut metadata) = open_metadata(b"", 2);
    let writer = BufferedWriter::with_capacity(&mut metadata, 2).unwrap();
    assert!(writer.is_empty());
    assert_eq!(writer.len(), 0);
}

#[test]
fn defaults_to_one_cache_row_for_rows_wider_than_the_target() {
    let row = vec![7; 64 * 1024 + 1];
    let (_directory, _path, mut metadata) = open_metadata(&row, row.len() as u64);

    {
        let mut writer = BufferedWriter::new(&mut metadata).unwrap();
        assert_eq!(writer.len(), 1);
        writer.write_row(0, &row).unwrap();
    }

    assert_eq!(metadata.file_len().unwrap(), row.len() as u64);
}

#[test]
fn rejects_invalid_cache_capacity() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);
    assert!(BufferedWriter::with_capacity(&mut metadata, 0).is_err());
    assert!(BufferedWriter::with_capacity(&mut metadata, usize::MAX).is_err());
    assert_eq!(metadata.file_len().unwrap(), 4);
}

#[test]
fn buffers_single_row_writes_and_appends() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcdef", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 4).unwrap();

    writer.write_row(1, b"XY").unwrap();
    writer.write_row(3, b"GH").unwrap();
    assert_eq!(writer.len(), 4);
    assert!(writer.write_row(0, b"x").is_err());
    assert!(writer.write_row(5, b"zz").is_err());
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abXYefGH");
}

#[test]
fn buffers_multiple_rows_and_appends() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 4).unwrap();

    writer.write_rows(1, b"WXYZ12").unwrap();
    assert_eq!(writer.len(), 4);
    assert!(writer.write_rows(0, b"x").is_err());
    assert!(writer.write_rows(5, b"xx").is_err());
    assert!(writer.write_rows(u64::MAX, b"xx").is_err());
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abWXYZ12");
}

#[test]
fn preserves_untouched_rows_in_cache_window() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcdefghij", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 5).unwrap();

    writer.write_row(1, b"12").unwrap();
    writer.write_row(2, b"34").unwrap();
    writer.write_row(1, b"XY").unwrap();
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abXY34ghij");
}

#[test]
fn flushes_before_starting_a_discontiguous_cache_range() {
    let (_directory, path, mut metadata) = open_metadata(b"abcdefghij", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 5).unwrap();

    writer.write_row(1, b"XY").unwrap();
    writer.write_row(3, b"UV").unwrap();

    assert_eq!(read_file(&path), b"abXYefghij");
    writer.write_row(0, b"12").unwrap();
    assert_eq!(read_file(&path), b"abXYefUVij");
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"12XYefUVij");
}

#[test]
fn flushes_on_cache_window_change() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcdefghij", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 2).unwrap();

    writer.write_row(0, b"XY").unwrap();
    writer.write_row(4, b"UV").unwrap();
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"XYcdefghUV");
}

#[test]
fn flushes_when_cache_is_full() {
    let (_directory, path, mut metadata) = open_metadata(b"abcd", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 2).unwrap();

    writer.write_row(0, b"XY").unwrap();
    assert_eq!(read_file(&path), b"abcd");
    writer.write_row(1, b"ZW").unwrap();
    assert_eq!(read_file(&path), b"XYZW");
}

#[test]
fn flushes_explicitly_and_is_idempotent() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();

    writer.write_row(1, b"XY").unwrap();
    writer.flush().unwrap();
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abXY");
}

#[test]
fn flushes_pending_rows_on_drop() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);

    {
        let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();
        writer.write_row(1, b"XY").unwrap();
    }

    assert_eq!(read_rows(&mut metadata), b"abXY");
}

#[test]
fn writes_oversized_block_directly_after_flush() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcdefgh", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 2).unwrap();

    writer.write_row(0, b"XY").unwrap();
    writer.write_rows(1, b"123456").unwrap();
    assert_eq!(writer.len(), 4);
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"XY123456");
}

#[test]
fn moves_fitting_block_to_new_window() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcdefghijkl", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 4).unwrap();

    writer.write_row(0, b"XY").unwrap();
    writer.write_rows(3, b"1234").unwrap();
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"XYcdef1234kl");
}

#[test]
fn validates_empty_block_ranges() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 2).unwrap();

    writer.write_rows(1, b"").unwrap();
    writer.write_rows(2, b"").unwrap();
    assert!(writer.write_rows(3, b"").is_err());
    assert_eq!(writer.len(), 2);
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abcd");
}

#[test]
fn retains_pending_rows_after_flush_error() {
    let (_directory, path, mut metadata) = open_metadata(b"abcdefgh", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();
    writer.write_row(2, b"XY").unwrap();

    replace_file(&path, b"ab");
    assert!(writer.flush().is_err());
    assert_eq!(writer.len(), 4);

    replace_file(&path, b"abcdefgh");
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abcdXYgh");
}

#[test]
fn propagates_oversized_block_write_error() {
    let (_directory, path, mut metadata) = open_metadata(b"abcdefgh", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 1).unwrap();
    replace_file(&path, b"ab");

    assert!(writer.write_rows(2, b"1234").is_err());
    assert_eq!(writer.len(), 4);
}

#[test]
fn appends_without_inserting_rows() {
    let (_directory, _path, mut metadata) = open_metadata(b"abcd", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();

    writer.write_row(2, b"EF").unwrap();
    assert_eq!(writer.len(), 3);
    writer.write_rows(3, b"GHIJ").unwrap();
    assert_eq!(writer.len(), 5);
    assert!(writer.write_row(6, b"zz").is_err());
    writer.flush().unwrap();

    drop(writer);
    assert_eq!(read_rows(&mut metadata), b"abcdEFGHIJ");
}

#[test]
fn keeps_writer_row_count_and_rejects_external_changes() {
    let (_directory, path, mut metadata) = open_metadata(b"abcdefgh", 2);
    let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();
    writer.write_row(2, b"XY").unwrap();
    replace_file(&path, b"ab");

    assert_eq!(writer.len(), 4);
    assert!(writer.flush().is_err());
    assert_eq!(writer.len(), 4);
}

#[test]
fn drop_does_not_panic_when_flush_fails() {
    let (_directory, path, mut metadata) = open_metadata(b"abcdefgh", 2);

    {
        let mut writer = BufferedWriter::with_capacity(&mut metadata, 3).unwrap();
        writer.write_row(2, b"XY").unwrap();
        replace_file(&path, b"ab");
    }
}
