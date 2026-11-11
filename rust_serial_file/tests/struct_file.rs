use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    sync::Arc,
};

use rust_serial_file::{
    adapter::{
        abstract_data_container::SerialDataContainer, abstract_serial_read::SerialDataReader,
        abstract_serial_write::SerialDataWriter,
    },
    file::{
        file_lock::FileLock,
        file_metadata::FileMetadata,
        struct_file::{StructFile, StructReadOnlyFile, StructWriteFile},
    },
    serialized_struct::{self, DecodeRaw, EncodeRaw, SerializedStruct},
};
use tempfile::tempdir;

#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq, PartialOrd)]
struct Record {
    key: u32,
    value: u32,
}

fn record(key: u32) -> Record {
    Record {
        key,
        value: key * 10,
    }
}

fn open_metadata(
    bytes: &[u8],
    element_size: u64,
    data_offset: u64,
) -> (tempfile::TempDir, std::path::PathBuf, FileMetadata) {
    let directory = tempdir().unwrap();
    let path = directory.path().join("records.bin");
    File::create(&path).unwrap().write_all(bytes).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let metadata = FileMetadata::new(file, element_size, data_offset).unwrap();
    (directory, path, metadata)
}

fn struct_file(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf, StructFile<Record>) {
    let (directory, path, metadata) = open_metadata(bytes, Record::SIZE as u64, 0);
    (directory, path, StructFile::new(metadata).unwrap())
}

fn encoded(records: &[Record]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        let mut row = vec![0; Record::SIZE];
        assert_eq!(record.try_encode(&mut row).unwrap(), Record::SIZE);
        bytes.extend(row);
    }
    bytes
}

fn read_file(path: &std::path::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn constructs_from_file_metadata() {
    let expected = encoded(&[record(1), record(2)]);
    let (_directory, _path, metadata) = open_metadata(&expected, Record::SIZE as u64, 0);

    let file = StructFile::<Record>::new(metadata).unwrap();
    assert_eq!(file.serial_len(), 2);
    assert_eq!(file.file_metadata().element_size(), Record::SIZE as u64);

    let metadata = file.into_file_metadata();
    assert_eq!(metadata.element_count().unwrap(), 2);
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

#[test]
fn rejects_invalid_serialized_row_width() {
    let (_directory, _path, metadata) = open_metadata(b"abcd", 2, 0);
    assert!(StructFile::<u32>::new(metadata).is_err());

    let (_directory, _path, metadata) = open_metadata(b"x", 1, 0);
    assert!(StructFile::<ZeroSize>::new(metadata).is_err());
}

#[test]
fn opens_or_creates_typed_file() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("typed.bin");
    {
        let file = StructFile::<Record>::open_or_create(&path, 3).unwrap();
        assert_eq!(file.serial_len(), 0);
        assert_eq!(file.file_metadata().data_offset(), 3);
    }
    assert_eq!(read_file(&path), [0; 0]);

    let records = encoded(&[record(7)]);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(std::io::SeekFrom::Start(3)).unwrap();
    file.write_all(&records).unwrap();

    let typed = StructFile::<Record>::open_or_create(&path, 3).unwrap();
    assert_eq!(typed.serial_len(), 1);
}

#[test]
fn reads_and_replaces_typed_values() {
    let (_directory, _path, mut file) = struct_file(&encoded(&[record(1), record(2)]));

    assert_eq!(file.read_value_at(1).unwrap(), record(2));
    file.write_value_at(0, &record(9)).unwrap();

    assert_eq!(file.read_value_at(0).unwrap(), record(9));
    assert!(file.read_value_at(2).is_err());
    assert!(file.write_value_at(2, &record(3)).is_err());
}

const LARGE_RECORD_SIZE: usize = 1025;

struct LargeRecord([u8; LARGE_RECORD_SIZE]);

impl EncodeRaw for LargeRecord {
    fn encode(&self, data: &mut [u8]) -> usize {
        data[..LARGE_RECORD_SIZE].copy_from_slice(&self.0);
        LARGE_RECORD_SIZE
    }
}

impl DecodeRaw for LargeRecord {
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self {
        let start = *offset;
        *offset += LARGE_RECORD_SIZE;
        Self(data[start..*offset].try_into().unwrap())
    }
}

impl SerializedStruct for LargeRecord {
    const SIZE: usize = LARGE_RECORD_SIZE;
}

#[test]
fn reads_rows_larger_than_the_stack_buffer() {
    let bytes = [0x5a; LargeRecord::SIZE];
    let (_directory, _path, metadata) = open_metadata(&bytes, LargeRecord::SIZE as u64, 0);
    let file = StructFile::<LargeRecord>::new(metadata).unwrap();

    assert_eq!(file.read_value_at(0).unwrap().0, bytes);
}

#[test]
fn writes_rows_larger_than_the_stack_buffer() {
    let original = vec![0x11; LargeRecord::SIZE * 2];
    let (_directory, _path, metadata) = open_metadata(&original, LargeRecord::SIZE as u64, 0);
    let mut file = StructFile::<LargeRecord>::new(metadata).unwrap();
    let replacement = LargeRecord([0x22; LARGE_RECORD_SIZE]);
    let appended = LargeRecord([0x33; LARGE_RECORD_SIZE]);
    let inserted = LargeRecord([0x44; LARGE_RECORD_SIZE]);

    file.write_value_at(0, &replacement).unwrap();
    file.serial_write(appended).unwrap();
    file.serial_insert(1, inserted).unwrap();

    assert_eq!(file.read_value_at(0).unwrap().0, [0x22; LARGE_RECORD_SIZE]);
    assert_eq!(file.read_value_at(1).unwrap().0, [0x44; LARGE_RECORD_SIZE]);
    assert_eq!(file.read_value_at(2).unwrap().0, [0x11; LARGE_RECORD_SIZE]);
    assert_eq!(file.read_value_at(3).unwrap().0, [0x33; LARGE_RECORD_SIZE]);
}

struct IncorrectEncoder;

impl EncodeRaw for IncorrectEncoder {
    fn encode(&self, data: &mut [u8]) -> usize {
        data[0] = 1;
        0
    }
}

impl DecodeRaw for IncorrectEncoder {
    fn from_le_bytes_slices(_data: &[u8], _offset: &mut usize) -> Self {
        Self
    }
}

impl SerializedStruct for IncorrectEncoder {
    const SIZE: usize = 1;
}

struct FailingEncoder;

impl EncodeRaw for FailingEncoder {
    fn encode(&self, _data: &mut [u8]) -> usize {
        0
    }
}

impl DecodeRaw for FailingEncoder {
    fn from_le_bytes_slices(_data: &[u8], _offset: &mut usize) -> Self {
        Self
    }
}

impl SerializedStruct for FailingEncoder {
    const SIZE: usize = 1;

    fn try_encode(&self, _data: &mut [u8]) -> anyhow::Result<usize> {
        anyhow::bail!("intentional encode failure")
    }
}

#[test]
fn rejects_incorrect_encoded_length() {
    let (_directory, path, metadata) = open_metadata(b"\x07", 1, 0);
    let mut file = StructFile::<IncorrectEncoder>::new(metadata).unwrap();

    assert!(file.write_value_at(0, &IncorrectEncoder).is_err());
    assert_eq!(read_file(&path), [7]);
    assert!(file.write_multiple(&[IncorrectEncoder]).is_err());
    assert_eq!(read_file(&path), [7]);
}

#[test]
fn encode_errors_do_not_change_collection_length() {
    let (_directory, path, metadata) = open_metadata(b"\x07", 1, 0);
    let mut file = StructFile::<FailingEncoder>::new(metadata).unwrap();

    assert!(file.serial_write(FailingEncoder).is_err());
    assert_eq!(file.serial_len(), 1);
    assert_eq!(read_file(&path), [7]);
    assert!(file.write_multiple(&[FailingEncoder]).is_err());
    assert_eq!(file.serial_len(), 1);
    assert_eq!(read_file(&path), [7]);
}

struct FailingDecoder;

impl EncodeRaw for FailingDecoder {
    fn encode(&self, data: &mut [u8]) -> usize {
        data[0] = 1;
        1
    }
}

impl DecodeRaw for FailingDecoder {
    fn from_le_bytes_slices(_data: &[u8], _offset: &mut usize) -> Self {
        Self
    }
}

impl SerializedStruct for FailingDecoder {
    const SIZE: usize = 1;

    fn try_decode(_data: &[u8]) -> anyhow::Result<Self> {
        anyhow::bail!("intentional decode failure")
    }
}

#[test]
fn decode_errors_are_reported_without_removing_rows() {
    let (_directory, _path, metadata) = open_metadata(b"\x01", 1, 0);
    let mut file = StructFile::<FailingDecoder>::new(metadata).unwrap();

    assert!(file.read_value_at(0).is_err());
    assert!(file.serial_remove_get(0).is_err());
    assert_eq!(file.serial_len(), 1);
    let mut iterator = file.serial_iter().unwrap();
    assert!(iterator.next().unwrap().is_err());
    assert!(iterator.next().is_none());
}

#[test]
fn iterates_typed_rows_in_order() {
    let records = [record(1), record(2), record(3)];
    let (_directory, _path, file) = struct_file(&encoded(&records));

    let values = file
        .serial_iter()
        .unwrap()
        .map(|value| value.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values, records);
}

#[test]
fn iterator_reads_across_buffer_cache_windows() {
    let records = (0..8_195).map(record).collect::<Vec<_>>();
    let (_directory, _path, file) = struct_file(&encoded(&records));

    let values = file
        .serial_iter()
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();

    assert_eq!(values, records);
}

#[test]
fn iterator_reports_row_errors() {
    let (_directory, path, file) = struct_file(&encoded(&[record(1), record(2)]));
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(Record::SIZE as u64)
        .unwrap();

    let mut iterator = file.serial_iter().unwrap();
    assert_eq!(iterator.next().unwrap().unwrap(), record(1));
    assert!(iterator.next().unwrap().is_err());
    assert!(iterator.next().is_none());
}

#[test]
fn serial_write_appends_values() {
    let (_directory, _path, mut file) = struct_file(&encoded(&[record(1)]));

    file.serial_write(record(2)).unwrap();
    file.serial_write(record(3)).unwrap();

    assert_eq!(file.serial_len(), 3);
    assert_eq!(file.read_value_at(2).unwrap(), record(3));
}

#[test]
fn serial_insert_shifts_rows_and_checks_bounds() {
    let (_directory, _path, mut file) = struct_file(&encoded(&[record(1), record(3)]));

    file.serial_insert(0, record(0)).unwrap();
    file.serial_insert(2, record(2)).unwrap();
    file.serial_insert(4, record(4)).unwrap();
    assert!(file.serial_insert(6, record(6)).is_err());

    assert_eq!(file.serial_len(), 5);
    assert_eq!(
        file.serial_iter()
            .unwrap()
            .map(|value| value.unwrap())
            .collect::<Vec<_>>(),
        [record(0), record(1), record(2), record(3), record(4)]
    );
}

#[test]
fn serial_write_at_replaces_existing_value() {
    let (_directory, _path, mut file) = struct_file(&encoded(&[record(1), record(2)]));

    file.serial_write_at(1, record(8)).unwrap();
    assert!(file.serial_write_at(2, record(9)).is_err());

    assert_eq!(file.read_value_at(1).unwrap(), record(8));
}

#[test]
fn serial_remove_preserves_remaining_order() {
    let (_directory, _path, mut file) =
        struct_file(&encoded(&[record(0), record(1), record(2), record(3)]));

    file.serial_remove(1).unwrap();
    assert_eq!(file.serial_remove_get(1).unwrap(), record(2));
    assert_eq!(file.serial_remove_get(1).unwrap(), record(3));
    assert!(file.serial_remove(1).is_err());

    assert_eq!(file.serial_len(), 1);
    assert_eq!(file.read_value_at(0).unwrap(), record(0));
}

#[test]
fn rejects_out_of_range_replace_and_remove_get_without_mutation() {
    let records = [record(1), record(2)];
    let initial_bytes = encoded(&records);
    let (_directory, path, mut file) = struct_file(&initial_bytes);

    assert!(file.write_value_at(file.serial_len(), &record(3)).is_err());
    assert!(file.serial_remove_get(file.serial_len()).is_err());

    assert_eq!(file.serial_len(), records.len());
    assert_eq!(
        file.serial_iter()
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        records
    );
    assert_eq!(read_file(&path), initial_bytes);
}

#[test]
fn serial_remove_shifts_rows_across_multiple_chunks() {
    let records = (0..8_195).map(record).collect::<Vec<_>>();
    let (_directory, _path, mut file) = struct_file(&encoded(&records));

    file.serial_remove(0).unwrap();

    assert_eq!(file.serial_len(), records.len() - 1);
    assert_eq!(file.read_value_at(0).unwrap(), record(1));
    assert_eq!(
        file.read_value_at(file.serial_len() - 1).unwrap(),
        record(8_194)
    );
}

#[test]
fn serial_clear_and_truncate_preserve_metadata() {
    let prefix = b"meta";
    let mut bytes = prefix.to_vec();
    bytes.extend(encoded(&[record(1), record(2), record(3)]));
    let (_directory, path, metadata) = open_metadata(&bytes, Record::SIZE as u64, 4);
    let mut file = StructFile::<Record>::new(metadata).unwrap();

    file.serial_truncate(2).unwrap();
    assert_eq!(file.serial_len(), 2);
    file.serial_truncate(2).unwrap();
    file.serial_truncate(9).unwrap();
    assert_eq!(file.serial_len(), 2);
    file.serial_clear().unwrap();
    assert!(file.serial_is_empty());
    assert_eq!(&read_file(&path)[..4], prefix);
    assert_eq!(read_file(&path).len(), 4);
}

#[test]
fn container_operations_use_typed_records() {
    let (_directory, _path, mut file) = struct_file(&encoded(&[record(1), record(3)]));

    file.serial_auto_insert(record(2)).unwrap();
    file.serial_update_by_key(&record(3), |current| {
        let mut current = current.unwrap();
        current.value = 99;
        Ok(current)
    })
    .unwrap();
    assert_eq!(file.serial_find(&record(2)).unwrap(), Some(1));
    assert_eq!(file.read_value_at(2).unwrap().value, 99);
}

#[test]
fn maintains_infallible_collection_length() {
    let (_directory, path, mut file) = struct_file(&encoded(&[record(1)]));

    assert_eq!(file.serial_len(), 1);
    assert!(!file.serial_is_empty());
    file.serial_write(record(2)).unwrap();
    assert_eq!(file.serial_len(), 2);

    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(file.read_value_at(0).is_err());
    assert_eq!(file.serial_len(), 2);
}

#[test]
fn exposes_and_recovers_owned_metadata() {
    let (_directory, _path, metadata) = open_metadata(&[], Record::SIZE as u64, 0);
    let file = StructFile::<Record>::new(metadata).unwrap();

    assert_eq!(file.file_metadata().element_size(), Record::SIZE as u64);
    assert_eq!(file.into_file_metadata().element_count().unwrap(), 0);
}

#[test]
fn preserves_existing_file_layout() {
    let prefix = b"header";
    let records = [record(1), record(2)];
    let mut bytes = prefix.to_vec();
    bytes.extend(encoded(&records));
    let (_directory, _path, metadata) =
        open_metadata(&bytes, Record::SIZE as u64, prefix.len() as u64);
    let mut file = StructFile::<Record>::new(metadata).unwrap();

    assert_eq!(file.read_value_at(0).unwrap(), record(1));
    file.serial_write(record(3)).unwrap();
    let metadata = file.into_file_metadata();
    assert_eq!(metadata.element_count().unwrap(), 3);
    let raw = read_file_from_metadata(metadata);
    assert_eq!(&raw[..prefix.len()], prefix);
    assert_eq!(
        &raw[prefix.len()..],
        encoded(&[record(1), record(2), record(3)])
    );
}

fn read_file_from_metadata(metadata: FileMetadata) -> Vec<u8> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("copy.bin");
    let mut output = File::create(&path).unwrap();
    let metadata = metadata;
    let mut prefix = vec![0; metadata.data_offset() as usize];
    metadata.get_raw_metadata(&mut prefix).unwrap();
    output.write_all(&prefix).unwrap();
    for row in metadata.rows().unwrap() {
        output.write_all(&row.unwrap()).unwrap();
    }
    read_file(&path)
}

#[test]
fn write_multiple_replaces_and_appends_in_slice_order() {
    let initial = [record(4), record(5), record(6)];
    let (_directory, _path, mut file) = struct_file(&encoded(&initial));

    file.write_multiple(&[record(2)]).unwrap();
    assert_eq!(file.serial_len(), 3);
    assert_eq!(
        file.serial_iter()
            .unwrap()
            .map(|value| value.unwrap())
            .collect::<Vec<_>>(),
        [record(2), record(5), record(6)]
    );

    file.write_multiple(&[record(10), record(11), record(12)])
        .unwrap();
    assert_eq!(file.serial_len(), 3);
    assert_eq!(
        file.serial_iter()
            .unwrap()
            .map(|value| value.unwrap())
            .collect::<Vec<_>>(),
        [record(10), record(11), record(12)]
    );

    file.write_multiple(&[record(8), record(7), record(9), record(3)])
        .unwrap();
    assert_eq!(file.serial_len(), 4);
    assert_eq!(
        file.serial_iter()
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        [record(8), record(7), record(9), record(3)]
    );
    file.write_multiple(&[]).unwrap();
    assert_eq!(file.serial_len(), 4);
}

/// Reads a `StructFile` through a shared reference only.
fn read_shared(file: &StructFile<Record>) {
    let mut iter = file.serial_iter().unwrap();
    assert_eq!(iter.next().unwrap().unwrap(), record(1));

    // Indexed reads and searches while the iterator is alive.
    assert_eq!(file.serial_get(3).unwrap(), record(4));
    assert_eq!(file.serial_find(&record(3)).unwrap(), Some(2));
    assert_eq!(file.serial_get(3).unwrap(), file.serial_get(3).unwrap());

    let rest = iter.collect::<anyhow::Result<Vec<_>>>().unwrap();
    assert_eq!(rest, [record(2), record(3), record(4)]);
    assert_eq!(file.serial_first().unwrap(), record(1));
    assert_eq!(file.serial_last().unwrap(), record(4));
}

#[test]
fn struct_file_reads_through_a_shared_reference() {
    let bytes = encoded(&[record(1), record(2), record(3), record(4)]);
    let (_directory, _path, file) = struct_file(&bytes);

    let shared = &file;
    let also_shared = &file;
    read_shared(shared);
    assert_eq!(also_shared.read_value_at(0).unwrap(), record(1));
}

/// Reads every record through any read-only view.
fn read_records<V: SerialDataReader<Record>>(view: &V) -> Vec<Record> {
    view.serial_iter()
        .unwrap()
        .collect::<anyhow::Result<_>>()
        .unwrap()
}

#[test]
fn read_only_views_share_borrowed_metadata() {
    let bytes = encoded(&[record(1), record(2), record(3)]);
    let (_directory, _path, metadata) = open_metadata(&bytes, Record::SIZE as u64, 0);

    let first = StructReadOnlyFile::<Record>::new(&metadata).unwrap();
    let second = StructReadOnlyFile::<Record>::new(&metadata).unwrap();
    let mut iter = first.serial_iter().unwrap();
    assert_eq!(iter.next().unwrap().unwrap(), record(1));
    assert_eq!(second.serial_get(2).unwrap(), record(3));
    assert_eq!(second.serial_find(&record(2)).unwrap(), Some(1));
    assert_eq!(iter.next().unwrap().unwrap(), record(2));
    assert_eq!(first.file_metadata().element_size(), Record::SIZE as u64);
    assert!(std::ptr::eq(second.into_inner(), &metadata));

    let shared = Arc::new(metadata);
    let arc_view = StructFile::<Record, Arc<FileMetadata>>::new(Arc::clone(&shared)).unwrap();
    assert_eq!(read_records(&arc_view), [record(1), record(2), record(3)]);
}

#[test]
fn writable_view_changes_are_seen_by_later_views() {
    let bytes = encoded(&[record(1), record(4)]);
    let (_directory, _path, mut metadata) = open_metadata(&bytes, Record::SIZE as u64, 0);

    {
        let mut writer = StructWriteFile::<Record>::new(&mut metadata).unwrap();
        writer.serial_write(record(6)).unwrap();
        writer.serial_insert(1, record(2)).unwrap();
        writer.serial_auto_insert(record(5)).unwrap();
        writer.serial_remove(0).unwrap();
        assert_eq!(writer.serial_len(), 4);
    }
    assert_eq!(metadata.element_count().unwrap(), 4);

    let expected = [record(2), record(4), record(5), record(6)];
    let reader = StructReadOnlyFile::<Record>::new(&metadata).unwrap();
    assert_eq!(reader.serial_len(), 4);
    assert_eq!(read_records(&reader), expected);

    let owned = StructFile::<Record>::new(metadata).unwrap();
    assert_eq!(read_records(&owned), expected);
}

#[test]
fn as_read_only_copies_length_without_io() {
    let bytes = encoded(&[record(1), record(2)]);
    let (_directory, _path, mut metadata) = open_metadata(&bytes, Record::SIZE as u64, 0);

    {
        let mut writer = StructWriteFile::<Record>::new(&mut metadata).unwrap();
        writer.serial_write(record(3)).unwrap();
        let view = writer.as_read_only();
        assert_eq!(view.serial_len(), 3);
        assert_eq!(read_records(&view), [record(1), record(2), record(3)]);
    }

    let owned = StructFile::<Record>::new(metadata).unwrap();
    let view = owned.as_read_only();
    let other = owned.as_read_only();
    assert_eq!(view.serial_len(), owned.serial_len());
    assert_eq!(other.serial_last().unwrap(), record(3));
}

#[test]
fn views_work_through_file_lock_guards() {
    let bytes = encoded(&[record(1)]);
    let (_directory, _path, metadata) = open_metadata(&bytes, Record::SIZE as u64, 0);
    let lock = FileLock::new(metadata);

    {
        let mut guard = lock.write().unwrap();
        let mut writer = StructWriteFile::<Record>::new(&mut *guard).unwrap();
        writer.serial_write(record(2)).unwrap();
        writer.serial_write(record(3)).unwrap();
    }

    let first_guard = lock.read().unwrap();
    let second_guard = lock.read().unwrap();
    let first = StructReadOnlyFile::<Record>::new(&*first_guard).unwrap();
    let second = StructReadOnlyFile::<Record>::new(&*second_guard).unwrap();
    assert_eq!(read_records(&first), [record(1), record(2), record(3)]);
    assert_eq!(second.serial_get(2).unwrap(), record(3));
}

#[test]
fn views_are_send_and_sync() {
    fn assert_send_sync<V: Send + Sync>() {}

    assert_send_sync::<StructFile<Record>>();
    assert_send_sync::<StructReadOnlyFile<'static, Record>>();
    assert_send_sync::<StructWriteFile<'static, Record>>();
}
