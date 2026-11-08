use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

use rust_serial_file::file::file_metadata::FileMetadata;
use tempfile::tempdir;

fn open_metadata(
    bytes: &[u8],
    element_size: u64,
    data_offset: u64,
) -> (tempfile::TempDir, FileMetadata) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    let mut file = File::create(&path).unwrap();
    file.write_all(bytes).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let metadata = FileMetadata::new(file, element_size, data_offset).unwrap();
    (directory, metadata)
}

fn read_all(metadata: &mut FileMetadata) -> Vec<u8> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("copy.bin");
    let mut output = File::create(&path).unwrap();
    {
        let mut rows = metadata.rows().unwrap();
        for row in &mut rows {
            output.write_all(&row.unwrap()).unwrap();
        }
    }
    let mut bytes = Vec::new();
    File::open(path).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn constructs_from_owned_file() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    let file = File::create(path).unwrap();

    let metadata = FileMetadata::new(file, 4, 0).unwrap();

    assert_eq!(metadata.file_len().unwrap(), 0);
}

#[test]
fn reports_physical_file_length() {
    let (_directory, metadata) = open_metadata(b"metadata123", 3, 0);
    assert_eq!(metadata.file_len().unwrap(), 11);
}

#[test]
fn counts_fixed_size_elements() {
    let (_directory, metadata) = open_metadata(b"abcdef", 2, 0);
    assert_eq!(metadata.element_count().unwrap(), 3);
}

#[test]
fn excludes_leading_metadata_from_element_count() {
    let (_directory, metadata) = open_metadata(b"metaabcdef", 2, 4);
    assert_eq!(metadata.element_count().unwrap(), 3);
}

#[test]
fn handles_offset_beyond_end_and_partial_elements() {
    let (_directory, short) = open_metadata(b"abc", 2, 4);
    assert_eq!(short.element_count().unwrap(), 0);

    let (_directory, partial) = open_metadata(b"mabcde", 2, 1);
    assert_eq!(partial.element_count().unwrap(), 2);
}

#[test]
fn rejects_zero_element_size() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    let file = File::create(path).unwrap();
    assert!(FileMetadata::new(file, 0, 0).is_err());
}

#[test]
fn observes_length_changes_from_caller_managed_handle() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    let file = File::create(&path).unwrap();
    let metadata = FileMetadata::new(file, 2, 0).unwrap();
    let mut external = OpenOptions::new().write(true).open(path).unwrap();

    external.write_all(b"abcd").unwrap();

    assert_eq!(metadata.file_len().unwrap(), 4);
    assert_eq!(metadata.element_count().unwrap(), 2);
}

#[test]
fn creates_missing_file_from_path() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("new.bin");

    let metadata = FileMetadata::open_or_create(&path, 4, 0).unwrap();

    assert_eq!(metadata.file_len().unwrap(), 0);
    assert!(path.exists());
}

#[test]
fn opens_existing_file_without_truncating() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("existing.bin");
    File::create(&path).unwrap().write_all(b"existing").unwrap();

    let metadata = FileMetadata::open_or_create(&path, 2, 0).unwrap();

    assert_eq!(metadata.file_len().unwrap(), 8);
    assert_eq!(std::fs::read(path).unwrap(), b"existing");
}

#[test]
fn loads_exact_raw_metadata() {
    let (_directory, mut metadata) = open_metadata(b"metaabcd", 2, 4);
    let mut prefix = [0; 4];
    metadata.get_raw_metadata(&mut prefix).unwrap();
    assert_eq!(&prefix, b"meta");
    assert!(metadata.get_raw_metadata(&mut [0; 3]).is_err());

    let (_directory, mut short) = open_metadata(b"abc", 2, 4);
    assert!(short.get_raw_metadata(&mut [0; 4]).is_err());
}

#[test]
fn updates_metadata_without_losing_rows() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path)
        .unwrap()
        .write_all(b"metaabcdefX")
        .unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut metadata = FileMetadata::new(file, 2, 4).unwrap();

    metadata.update_raw_metadata(b"HEAD").unwrap();
    assert_eq!(metadata.data_offset(), 4);
    assert_eq!(std::fs::read(&path).unwrap(), b"HEADabcdefX");
    assert_eq!(read_all(&mut metadata), b"abcdef");

    metadata.update_raw_metadata(b"H").unwrap();
    assert_eq!(metadata.data_offset(), 1);
    assert_eq!(metadata.file_len().unwrap(), 8);
    assert_eq!(std::fs::read(&path).unwrap(), b"HabcdefX");
    assert_eq!(read_all(&mut metadata), b"abcdef");

    metadata.update_raw_metadata(b"LONGER").unwrap();
    assert_eq!(metadata.data_offset(), 6);
    assert_eq!(metadata.file_len().unwrap(), 13);
    assert_eq!(std::fs::read(&path).unwrap(), b"LONGERabcdefX");
    assert_eq!(read_all(&mut metadata), b"abcdef");
}

#[test]
fn truncates_rows_from_back() {
    let (_directory, mut metadata) = open_metadata(b"MabcdeX", 2, 1);

    metadata.truncate_rows(2).unwrap();

    assert_eq!(metadata.file_len().unwrap(), 5);
    assert_eq!(read_all(&mut metadata), b"abcd");
    assert!(metadata.truncate_rows(3).is_err());
    metadata.truncate_rows(0).unwrap();
    assert_eq!(metadata.file_len().unwrap(), 1);
}

#[test]
fn truncates_rows_from_front() {
    let (_directory, mut metadata) = open_metadata(b"MabcdefX", 2, 1);

    metadata.truncate_rows_front(2).unwrap();

    assert_eq!(metadata.file_len().unwrap(), 5);
    assert_eq!(read_all(&mut metadata), b"cdef");
    assert!(metadata.truncate_rows_front(3).is_err());
}

#[test]
fn reads_and_writes_rows_without_buffering() {
    let (_directory, mut metadata) = open_metadata(b"Mabcdef", 2, 1);
    let mut row = [0; 2];
    metadata.read_row(1, &mut row).unwrap();
    assert_eq!(&row, b"cd");

    metadata.write_row(1, b"XY").unwrap();
    assert_eq!(read_all(&mut metadata), b"abXYef");
    assert_eq!(metadata.file_len().unwrap(), 7);
    assert!(metadata.read_row(0, &mut [0; 1]).is_err());
    assert!(metadata.write_row(0, b"x").is_err());
    assert!(metadata.write_row(3, b"zz").is_err());
}

#[test]
fn exposes_read_only_layout_metadata() {
    let (_directory, mut metadata) = open_metadata(b"metaabcd", 2, 4);
    assert_eq!(metadata.data_offset(), 4);
    assert_eq!(metadata.element_size(), 2);

    metadata.update_raw_metadata(b"prefix").unwrap();

    assert_eq!(metadata.data_offset(), 6);
    assert_eq!(metadata.element_size(), 2);
}

#[test]
fn iterates_all_rows_into_reusable_buffer() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    let mut iter = metadata.iter_rows().unwrap();
    let mut row = [0; 2];
    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"ab");
    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"cd");
    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"ef");
    assert!(!iter.next_into(&mut row).unwrap());
}

#[test]
fn iterates_selected_row_range() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    let mut iter = metadata.iter_rows_range(1, 1).unwrap();
    let mut row = [0; 2];
    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"cd");
    assert!(!iter.next_into(&mut row).unwrap());

    let mut empty = metadata.iter_rows_range(3, 0).unwrap();
    assert!(!empty.next_into(&mut row).unwrap());
    assert!(metadata.iter_rows_range(u64::MAX, 1).is_err());
}

#[test]
fn validates_row_iterator_buffer_size() {
    {
        let (_directory, mut metadata) = open_metadata(b"abcd", 2, 0);
        let mut iter = metadata.iter_rows().unwrap();
        assert!(iter.next_into(&mut [0; 1]).is_err());
    }
    {
        let (_directory, mut metadata) = open_metadata(b"abcd", 2, 0);
        let mut iter = metadata.iter_rows().unwrap();
        let mut row = [0; 2];
        assert!(iter.next_into(&mut row).unwrap());
        assert!(iter.next_into(&mut row).unwrap());
        assert!(!iter.next_into(&mut [0; 1]).unwrap());
    }
}

#[test]
fn standard_iterator_yields_all_rows() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    let rows = metadata
        .rows()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows, [b"ab".to_vec(), b"cd".to_vec(), b"ef".to_vec()]);
}

#[test]
fn standard_iterator_yields_selected_range() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    let rows = metadata
        .rows_range(1, 1)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows, [b"cd".to_vec()]);
    assert!(metadata.rows_range(3, 0).unwrap().next().is_none());
    assert!(metadata.rows_range(3, 1).is_err());
}

#[test]
fn standard_iterator_stops_after_read_error() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(b"abcd").unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut metadata = FileMetadata::new(file, 2, 0).unwrap();
    let mut rows = metadata.rows().unwrap();
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(0)
        .unwrap();

    assert!(rows.next().unwrap().is_err());
    assert!(rows.next().is_none());
}

#[test]
fn reads_contiguous_row_blocks() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    let mut rows = [0; 4];
    metadata.read_rows(1, &mut rows).unwrap();
    assert_eq!(&rows, b"cdef");
    metadata.read_rows(3, &mut []).unwrap();
    assert!(metadata.read_rows(3, &mut [0; 2]).is_err());
    assert!(metadata.read_rows(0, &mut [0; 3]).is_err());
}

#[test]
fn writes_contiguous_row_blocks() {
    let (_directory, mut metadata) = open_metadata(b"abcdef", 2, 0);
    metadata.write_rows(1, b"WXYZ").unwrap();
    assert_eq!(read_all(&mut metadata), b"abWXYZ");
    metadata.write_rows(3, &[]).unwrap();
    assert_eq!(metadata.file_len().unwrap(), 6);
    assert!(metadata.write_rows(3, b"xx").is_err());
    assert!(metadata.write_rows(0, b"x").is_err());
}

#[test]
fn appends_rows_and_discards_partial_suffix() {
    let (_directory, mut metadata) = open_metadata(b"MabcdX", 2, 1);
    metadata.append_row(b"YZ").unwrap();
    assert_eq!(metadata.file_len().unwrap(), 7);
    assert_eq!(read_all(&mut metadata), b"abcdYZ");

    metadata.append_row(b"12").unwrap();
    assert_eq!(read_all(&mut metadata), b"abcdYZ12");
    assert!(metadata.append_row(b"x").is_err());

    let (_directory, mut short_prefix) = open_metadata(b"ab", 2, 4);
    short_prefix.append_row(b"XY").unwrap();
    assert_eq!(short_prefix.file_len().unwrap(), 6);
    assert_eq!(read_all(&mut short_prefix), b"XY");
    let mut prefix = [0; 4];
    short_prefix.get_raw_metadata(&mut prefix).unwrap();
    assert_eq!(prefix, [b'a', b'b', 0, 0]);
}

#[test]
fn inserts_rows_and_discards_partial_suffix() {
    let (_directory, mut metadata) = open_metadata(b"MabcdefX", 2, 1);
    metadata.insert_row(1, b"XY").unwrap();
    assert_eq!(metadata.file_len().unwrap(), 9);
    assert_eq!(read_all(&mut metadata), b"abXYcdef");

    metadata.insert_row(4, b"zz").unwrap();
    assert_eq!(read_all(&mut metadata), b"abXYcdefzz");
    assert!(metadata.insert_row(6, b"no").is_err());
    assert!(metadata.insert_row(0, b"x").is_err());

    let (_directory, mut short_prefix) = open_metadata(b"ab", 2, 4);
    short_prefix.insert_row(0, b"XY").unwrap();
    assert_eq!(short_prefix.file_len().unwrap(), 6);
    assert_eq!(read_all(&mut short_prefix), b"XY");
    let mut prefix = [0; 4];
    short_prefix.get_raw_metadata(&mut prefix).unwrap();
    assert_eq!(prefix, [b'a', b'b', 0, 0]);
}

#[test]
fn pads_metadata_before_replacement_write_validation() {
    let (_directory, mut metadata) = open_metadata(b"ab", 2, 4);
    assert!(metadata.write_row(0, b"XY").is_err());
    assert_eq!(metadata.file_len().unwrap(), 4);
    let mut prefix = [1; 4];
    metadata.get_raw_metadata(&mut prefix).unwrap();
    assert_eq!(prefix, [b'a', b'b', 0, 0]);

    let (_directory, mut metadata) = open_metadata(b"ab", 2, 4);
    assert!(metadata.write_rows(0, b"XY").is_err());
    assert_eq!(metadata.file_len().unwrap(), 4);
}

#[test]
fn reserves_rows_and_preserves_existing_offsets() {
    let (_directory, mut metadata) = open_metadata(b"MabcX", 2, 1);
    let original_len = metadata.file_len().unwrap();
    let mut original_second_row = [0; 2];
    metadata.read_row(1, &mut original_second_row).unwrap();
    assert_eq!(&original_second_row, b"cX");
    metadata.reserve_rows(0).unwrap();
    assert_eq!(metadata.file_len().unwrap(), original_len);
    metadata.reserve_rows(2).unwrap();
    assert_eq!(metadata.file_len().unwrap(), original_len);
    let mut second_row = [0; 2];
    metadata.read_row(1, &mut second_row).unwrap();
    assert_eq!(second_row, original_second_row);

    metadata.reserve_rows(4).unwrap();
    assert_eq!(metadata.file_len().unwrap(), 9);
    assert_eq!(metadata.element_count().unwrap(), 2);
    let mut row = [0; 2];
    metadata.read_row(0, &mut row).unwrap();
    assert_eq!(&row, b"ab");
    metadata.read_row(1, &mut row).unwrap();
    assert_eq!(&row, b"cX");
    assert!(metadata.read_row(2, &mut row).is_err());
    metadata.append_row(b"YZ").unwrap();
    assert_eq!(metadata.file_len().unwrap(), 9);
    assert_eq!(metadata.element_count().unwrap(), 3);
    metadata.insert_row(1, b"QQ").unwrap();
    assert_eq!(metadata.file_len().unwrap(), 9);
    assert_eq!(metadata.element_count().unwrap(), 4);
    metadata.read_row(1, &mut row).unwrap();
    assert_eq!(&row, b"QQ");
    metadata.read_row(2, &mut row).unwrap();
    assert_eq!(&row, b"cX");
    metadata.read_row(3, &mut row).unwrap();
    assert_eq!(&row, b"YZ");

    let (_directory, mut short_prefix) = open_metadata(b"ab", 2, 4);
    short_prefix.reserve_rows(2).unwrap();
    assert_eq!(short_prefix.file_len().unwrap(), 8);
    assert_eq!(short_prefix.element_count().unwrap(), 0);
    let mut prefix = [0; 4];
    short_prefix.get_raw_metadata(&mut prefix).unwrap();
    assert_eq!(prefix, [b'a', b'b', 0, 0]);
    short_prefix.append_row(b"XY").unwrap();
    assert_eq!(short_prefix.file_len().unwrap(), 8);
    assert_eq!(short_prefix.element_count().unwrap(), 1);
    assert_eq!(read_all(&mut short_prefix), b"XY");
}

#[test]
fn reopening_treats_reserved_rows_as_existing_rows() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("reserved.bin");
    let mut metadata = FileMetadata::open_or_create(&path, 2, 0).unwrap();

    metadata.append_row(b"AB").unwrap();
    metadata.reserve_rows(4).unwrap();
    assert_eq!(metadata.element_count().unwrap(), 1);

    let mut reopened = FileMetadata::open_or_create(&path, 2, 0).unwrap();
    assert_eq!(reopened.element_count().unwrap(), 4);
    let mut reserved_row = [1; 2];
    reopened.read_row(1, &mut reserved_row).unwrap();
    assert_eq!(reserved_row, [0; 2]);
}

#[test]
fn reports_row_reservation_failures() {
    let (_directory, mut metadata) = open_metadata(b"", 2, 0);
    assert!(metadata.reserve_rows(u64::MAX).is_err());
    assert_eq!(metadata.file_len().unwrap(), 0);

    let directory = tempdir().unwrap();
    let path = directory.path().join("read-only.bin");
    File::create(&path).unwrap();
    let file = File::open(path).unwrap();
    let mut read_only = FileMetadata::new(file, 2, 0).unwrap();
    assert!(read_only.reserve_rows(1).is_err());
    assert_eq!(read_only.file_len().unwrap(), 0);
}
