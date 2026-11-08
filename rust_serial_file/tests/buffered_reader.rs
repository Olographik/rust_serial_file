use std::{
    fs::{File, OpenOptions},
    io::Write,
};

use rust_serial_file::file::{buffered_reader::BufferedReader, file_metadata::FileMetadata};
use tempfile::tempdir;

fn open_metadata(bytes: &[u8], element_size: u64) -> (tempfile::TempDir, FileMetadata) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(bytes).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    (directory, FileMetadata::new(file, element_size, 0).unwrap())
}

#[test]
fn constructs_from_borrowed_file_metadata() {
    let (_directory, metadata) = open_metadata(b"abcd", 2);

    let reader = BufferedReader::new(&metadata).unwrap();
    assert_eq!(reader.len(), 2);
    drop(reader);
    assert_eq!(metadata.element_count().unwrap(), 2);
}

#[test]
fn supports_multiple_readers_from_shared_metadata() {
    let (_directory, metadata) = open_metadata(b"aabbccddeeff", 2);
    let metadata = &metadata;

    std::thread::scope(|scope| {
        for (index, expected) in [b"aa".as_slice(), b"bb", b"cc", b"dd", b"ee", b"ff"]
            .into_iter()
            .enumerate()
        {
            scope.spawn(move || {
                let mut reader = BufferedReader::with_capacity(metadata, 2).unwrap();
                assert_eq!(reader.read_row(index as u64).unwrap(), expected);
            });
        }
    });
}

#[test]
fn rejects_zero_cache_capacity() {
    let (_directory, metadata) = open_metadata(b"abcd", 2);
    assert!(BufferedReader::with_capacity(&metadata, 0).is_err());
}

#[test]
fn reads_raw_rows_and_reports_length() {
    let (_directory, metadata) = open_metadata(b"abcdef", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();

    assert_eq!(reader.len(), 3);
    assert!(!reader.is_empty());
    assert_eq!(reader.read_row(1).unwrap(), b"cd");
}

#[test]
fn reads_into_caller_buffer() {
    let (_directory, metadata) = open_metadata(b"abcdef", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();
    let mut row = [0; 2];

    reader.read_row_into(2, &mut row).unwrap();
    assert_eq!(&row, b"ef");
    assert!(reader.read_row_into(0, &mut [0; 1]).is_err());
    assert!(reader.read_row_into(0, &mut [0; 3]).is_err());
}

#[test]
fn refills_cache_for_misses_and_reuses_hits() {
    let (_directory, metadata) = open_metadata(b"aabbccddeeffgg", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 3).unwrap();

    assert_eq!(reader.read_row(2).unwrap(), b"cc");
    assert_eq!(reader.read_row(0).unwrap(), b"aa");
    assert_eq!(reader.read_row(5).unwrap(), b"ff");
    assert_eq!(reader.read_row(6).unwrap(), b"gg");
}

#[test]
fn uses_default_capacity_for_small_and_large_rows() {
    let (_directory, small_metadata) = open_metadata(b"abcd", 1);
    let mut small_reader = BufferedReader::new(&small_metadata).unwrap();
    assert_eq!(small_reader.read_row(3).unwrap(), b"d");

    let large_row = vec![7; 64 * 1024 + 1];
    let mut large_file = Vec::new();
    large_file.extend_from_slice(&large_row);
    large_file.extend_from_slice(&large_row);
    let (_directory, large_metadata) = open_metadata(&large_file, (64 * 1024 + 1) as u64);
    let mut large_reader = BufferedReader::new(&large_metadata).unwrap();
    assert_eq!(large_reader.read_row(1).unwrap(), large_row);
}

#[test]
fn uses_explicit_cache_capacity() {
    let (_directory, metadata) = open_metadata(b"aabbccddeeff", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();

    assert_eq!(reader.read_row(3).unwrap(), b"dd");
    assert_eq!(reader.read_row(2).unwrap(), b"cc");
    assert_eq!(reader.read_row(4).unwrap(), b"ee");
}

#[test]
fn rejects_out_of_range_rows() {
    let (_directory, metadata) = open_metadata(b"abcd", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 1).unwrap();

    assert!(reader.read_row(2).is_err());
    assert!(reader.read_row(u64::MAX).is_err());
    assert_eq!(reader.len(), 2);
}

#[test]
fn iterates_all_rows_in_order() {
    let (_directory, metadata) = open_metadata(b"aabbcc", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();
    let rows = reader.rows().collect::<anyhow::Result<Vec<_>>>().unwrap();
    assert_eq!(rows, [b"aa".to_vec(), b"bb".to_vec(), b"cc".to_vec()]);
}

#[test]
fn iterates_empty_reader() {
    let (_directory, metadata) = open_metadata(b"", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();
    assert!(reader.is_empty());
    assert!(reader.rows().next().is_none());
    assert!(!reader.iter_rows().next_into(&mut []).unwrap());
    assert!(reader.iter_row_slices().next().is_none());
}

#[test]
fn iterates_across_cache_windows() {
    let (_directory, metadata) = open_metadata(b"aabbccddeeffgg", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 3).unwrap();

    let rows = reader.rows().collect::<anyhow::Result<Vec<_>>>().unwrap();
    assert_eq!(
        rows,
        [
            b"aa".to_vec(),
            b"bb".to_vec(),
            b"cc".to_vec(),
            b"dd".to_vec(),
            b"ee".to_vec(),
            b"ff".to_vec(),
            b"gg".to_vec()
        ]
    );

    let mut iter = reader.iter_rows();
    let mut output = [0; 2];
    let mut custom_rows = Vec::new();
    while iter.next_into(&mut output).unwrap() {
        custom_rows.push(output);
    }
    assert_eq!(
        custom_rows,
        [*b"aa", *b"bb", *b"cc", *b"dd", *b"ee", *b"ff", *b"gg"]
    );

    let mut iter = reader.iter_row_slices();
    let mut slice_rows = Vec::new();
    while let Some(row) = iter.next() {
        slice_rows.push(row.unwrap().to_vec());
    }
    assert_eq!(slice_rows, rows);
}

#[test]
fn keeps_construction_row_count_snapshot() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(b"aabb").unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let metadata = FileMetadata::new(file, 2, 0).unwrap();
    let external = OpenOptions::new().write(true).open(path).unwrap();
    let mut reader = BufferedReader::with_capacity(&metadata, 1).unwrap();

    assert_eq!(reader.len(), 2);
    external.set_len(0).unwrap();
    assert_eq!(reader.len(), 2);
    assert!(reader.read_row(1).is_err());
}

#[test]
fn propagates_cache_refill_errors_without_reusing_partial_data() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(b"aabb").unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let metadata = FileMetadata::new(file, 2, 0).unwrap();
    let external = OpenOptions::new().write(true).open(path).unwrap();
    let mut reader = BufferedReader::with_capacity(&metadata, 1).unwrap();

    assert_eq!(reader.read_row(0).unwrap(), b"aa");
    external.set_len(0).unwrap();
    assert!(reader.read_row(1).is_err());
    assert!(reader.read_row(1).is_err());

    drop(reader);
    let (_directory, metadata) = open_metadata(b"aabb", 2);
    let path = _directory.path().join("records.bin");
    let external = OpenOptions::new().write(true).open(path).unwrap();
    let mut reader = BufferedReader::with_capacity(&metadata, 1).unwrap();
    external.set_len(0).unwrap();

    {
        let mut rows = reader.rows();
        assert!(rows.next().unwrap().is_err());
        assert!(rows.next().is_none());
    }

    {
        let mut rows = reader.iter_row_slices();
        assert!(rows.next().unwrap().is_err());
        assert!(rows.next().is_none());
    }
}

#[test]
fn reports_cache_size_failures() {
    let (_directory, metadata) = open_metadata(b"a", 1);
    assert!(BufferedReader::with_capacity(&metadata, usize::MAX).is_err());
}

#[test]
fn iterates_rows_into_reusable_buffer() {
    let (_directory, metadata) = open_metadata(b"aabbcc", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();
    let mut iter = reader.iter_rows_range(1, 2).unwrap();
    let mut row = [0; 2];

    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"bb");
    assert!(iter.next_into(&mut row).unwrap());
    assert_eq!(&row, b"cc");
    assert!(!iter.next_into(&mut []).unwrap());
    assert!(reader.iter_rows_range(2, 2).is_err());
    assert!(reader.iter_rows_range(u64::MAX, 1).is_err());
    assert!(reader.iter_rows().next_into(&mut [0; 1]).is_err());
}

#[test]
fn iterates_selected_owned_row_range() {
    let (_directory, metadata) = open_metadata(b"aabbccdd", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();

    let rows = reader
        .rows_range(1, 2)
        .unwrap()
        .collect::<anyhow::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows, [b"bb".to_vec(), b"cc".to_vec()]);
    assert!(reader.rows_range(3, 2).is_err());
    assert!(reader.rows_range(u64::MAX, 1).is_err());
}

#[test]
fn iterates_rows_as_cache_slices() {
    let (_directory, metadata) = open_metadata(b"aabbccdd", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 2).unwrap();

    let mut iter = reader.iter_row_slices_range(1, 3).unwrap();
    assert_eq!(iter.next().unwrap().unwrap(), b"bb");
    assert_eq!(iter.next().unwrap().unwrap(), b"cc");
    assert_eq!(iter.next().unwrap().unwrap(), b"dd");
    assert!(iter.next().is_none());
    assert!(reader.iter_row_slices_range(3, 2).is_err());
}

#[test]
fn resets_capacity_and_invalidates_cache() {
    let (directory, metadata) = open_metadata(b"aabbccdd", 2);
    let mut reader = BufferedReader::with_capacity(&metadata, 1).unwrap();

    assert_eq!(reader.read_row(0).unwrap(), b"aa");
    reader.reset_capacity(3).unwrap();
    assert_eq!(reader.read_row(2).unwrap(), b"cc");
    reader.reset_capacity(3).unwrap();
    assert_eq!(reader.read_row(1).unwrap(), b"bb");
    reader.reset_capacity(1).unwrap();
    assert_eq!(reader.len(), 4);

    assert!(reader.reset_capacity(0).is_err());
    assert_eq!(reader.read_row(1).unwrap(), b"bb");
    assert!(reader.reset_capacity(usize::MAX).is_err());
    let unallocatable_capacity = (isize::MAX as usize / 2) + 1;
    assert!(reader.reset_capacity(unallocatable_capacity).is_err());
    assert_eq!(reader.read_row(1).unwrap(), b"bb");
    assert_eq!(
        std::fs::read(directory.path().join("records.bin")).unwrap(),
        b"aabbccdd"
    );
}
