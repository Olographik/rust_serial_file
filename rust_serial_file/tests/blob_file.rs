use std::{fs, path::Path};

use rust_serial_file::file::blob_file::{BlobFile, BlobRef, BlobStats, compact_path};
use tempfile::{TempDir, tempdir};

fn blob_in(directory: &TempDir) -> (std::path::PathBuf, BlobFile) {
    let path = directory.path().join("values.blob");
    let blob = BlobFile::open_or_create(&path).unwrap();
    (path, blob)
}

/// Block header bytes: state then capacity, little-endian.
fn header(live: bool, capacity: u32) -> Vec<u8> {
    let mut bytes = u32::from(live).to_le_bytes().to_vec();
    bytes.extend_from_slice(&capacity.to_le_bytes());
    bytes
}

fn file_header() -> Vec<u8> {
    let mut bytes = b"RSFBLOB1".to_vec();
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes
}

fn open_error(path: &Path) -> String {
    format!("{:#}", BlobFile::open_or_create(path).unwrap_err())
}

#[test]
fn creates_and_reopens_header() {
    let directory = tempdir().unwrap();
    let (path, blob) = blob_in(&directory);
    assert_eq!(blob.path(), Some(path.as_path()));
    drop(blob);
    assert_eq!(fs::read(&path).unwrap(), file_header());

    let reopened = BlobFile::open_or_create(&path).unwrap();
    assert_eq!(
        reopened.stats(),
        BlobStats {
            file_len: 16,
            ..BlobStats::default()
        }
    );
}

#[test]
fn writes_and_reads_values() {
    let directory = tempdir().unwrap();
    let (path, mut blob) = blob_in(&directory);

    let hello = blob.write(b"hello").unwrap();
    assert_eq!(hello, BlobRef::new(24, 5));
    let eight = blob.write(b"12345678").unwrap();
    assert_eq!(eight, BlobRef::new(40, 8));
    let nine = blob.write(b"123456789").unwrap();
    assert_eq!(nine, BlobRef::new(56, 9));
    assert_eq!(blob.write(b"").unwrap(), BlobRef::EMPTY);

    let mut expected = file_header();
    expected.extend(header(true, 8));
    expected.extend(b"hello\0\0\0");
    expected.extend(header(true, 8));
    expected.extend(b"12345678");
    expected.extend(header(true, 16));
    expected.extend(b"123456789\0\0\0\0\0\0\0");
    assert_eq!(fs::read(&path).unwrap(), expected);

    let check = |blob: &BlobFile| {
        assert_eq!(blob.read(hello).unwrap(), b"hello");
        assert_eq!(blob.read(eight).unwrap(), b"12345678");
        assert_eq!(blob.read(nine).unwrap(), b"123456789");
        assert_eq!(blob.read(BlobRef::EMPTY).unwrap(), b"");
        let mut buffer = [0; 5];
        blob.read_into(hello, &mut buffer).unwrap();
        assert_eq!(&buffer, b"hello");
        assert!(blob.read_into(hello, &mut [0; 4]).is_err());
        blob.read_into(BlobRef::EMPTY, &mut []).unwrap();
    };
    check(&blob);
    drop(blob);
    check(&BlobFile::open_or_create(&path).unwrap());
}

#[test]
fn reads_strings() {
    let directory = tempdir().unwrap();
    let (_path, mut blob) = blob_in(&directory);
    let text = blob.write_str("température 21 °C").unwrap();
    assert_eq!(blob.read_string(text).unwrap(), "température 21 °C");
    assert_eq!(blob.read_string(BlobRef::EMPTY).unwrap(), "");

    let invalid = blob.write(&[0xFF, 0xFE]).unwrap();
    let error = format!("{:#}", blob.read_string(invalid).unwrap_err());
    assert!(error.contains("not valid UTF-8"), "{error}");
    assert_eq!(blob.read(invalid).unwrap(), [0xFF, 0xFE]);
}

#[test]
fn rejects_invalid_refs() {
    let directory = tempdir().unwrap();
    let (path, mut blob) = blob_in(&directory);
    let first = blob.write(b"0123456789abcdef").unwrap(); // payload 24..40
    let second = blob.write(b"second").unwrap(); // payload 48..56
    let freed = blob.write(b"freed").unwrap();
    blob.write(b"keeps the freed block in the middle").unwrap();
    blob.free(freed).unwrap();
    let before = fs::read(&path).unwrap();

    let invalid = [
        BlobRef::new(0, 1),             // inside the file header
        BlobRef::new(8, 1),             // header offset would be 0
        BlobRef::new(32, 4),            // inside the first payload
        BlobRef::new(16, 4),            // a block header, not a payload
        BlobRef::new(first.offset, 17), // longer than the capacity
        BlobRef::new(freed.offset, 5),  // a free block
        BlobRef::new(10_000, 1),        // beyond the end
        BlobRef::new(u64::MAX, 1),      // offset arithmetic must not overflow
    ];
    for reference in invalid {
        assert!(blob.read(reference).is_err(), "read {reference:?}");
        assert!(
            blob.read_into(reference, &mut [0; 1]).is_err(),
            "read_into {reference:?}"
        );
        assert!(
            blob.read_string(reference).is_err(),
            "read_string {reference:?}"
        );
        assert!(
            blob.update(reference, b"x").is_err(),
            "update {reference:?}"
        );
        assert!(blob.free(reference).is_err(), "free {reference:?}");
    }
    let message = format!(
        "{:#}",
        blob.read(BlobRef::new(first.offset, 17)).unwrap_err()
    );
    assert!(message.contains("exceeds the block capacity"), "{message}");
    let message = format!(
        "{:#}",
        blob.read(BlobRef::new(freed.offset, 5)).unwrap_err()
    );
    assert!(message.contains("is free"), "{message}");
    assert_eq!(fs::read(&path).unwrap(), before);

    // A shorter length than stored is valid: it reads a prefix.
    assert_eq!(blob.read(BlobRef::new(second.offset, 3)).unwrap(), b"sec");
}

#[test]
fn rejects_corrupt_files() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("corrupt.blob");
    let mut valid = file_header();
    valid.extend(header(true, 8));
    valid.extend([0; 8]);

    let cases: Vec<(Vec<u8>, &str)> = vec![
        (b"RSFBLOB".to_vec(), "shorter than its 16-byte header"),
        ([b"NOTABLOB".as_slice(), &valid[8..]].concat(), "bad magic"),
        (
            {
                let mut bytes = valid.clone();
                bytes[8] = 2;
                bytes
            },
            "unsupported blob file version 2",
        ),
        (
            {
                let mut bytes = valid.clone();
                bytes[12] = 1;
                bytes
            },
            "non-zero reserved bytes",
        ),
        (
            {
                let mut bytes = valid.clone();
                bytes[16] = 7;
                bytes
            },
            "invalid state 7",
        ),
        (valid[..30].to_vec(), "runs past the end"),
        ([valid.as_slice(), &[1, 2, 3]].concat(), "3 trailing bytes"),
    ];
    for (bytes, expected) in cases {
        fs::write(&path, &bytes).unwrap();
        let error = open_error(&path);
        assert!(
            error.contains(expected),
            "expected {expected:?} in {error:?}"
        );
    }

    fs::write(&path, &valid).unwrap();
    BlobFile::open_or_create(&path).unwrap();
}

#[test]
fn frees_and_merges_blocks() {
    let directory = tempdir().unwrap();
    let (path, mut blob) = blob_in(&directory);
    let refs: Vec<BlobRef> = (0..5).map(|i| blob.write(&[i; 8]).unwrap()).collect();
    // Five 16-byte blocks: headers at 16, 32, 48, 64, 80; file length 96.
    assert_eq!(blob.stats().file_len, 96);

    blob.free(refs[1]).unwrap();
    blob.free(refs[3]).unwrap();
    assert_eq!(blob.stats().free_blocks, 2);
    // Freeing the middle one merges previous, itself and next into one
    // block at 32 with capacity 8 + 8 + 8 + 8 + 8 = 40.
    blob.free(refs[2]).unwrap();
    let stats = blob.stats();
    assert_eq!(stats.free_blocks, 1);
    assert_eq!(stats.largest_free_block, 40);
    assert_eq!(stats.free_bytes, 48);
    assert_eq!(
        &fs::read(&path).unwrap()[32..40],
        header(false, 40).as_slice()
    );

    // Freeing the last block truncates the file, and the free block before
    // it goes too.
    blob.free(refs[4]).unwrap();
    assert_eq!(blob.stats().file_len, 32);
    assert_eq!(blob.stats().free_blocks, 0);
    assert_eq!(fs::metadata(&path).unwrap().len(), 32);
    assert_eq!(blob.read(refs[0]).unwrap(), [0; 8]);

    // Freeing the only block empties the file down to its header.
    blob.free(refs[0]).unwrap();
    assert_eq!(fs::read(&path).unwrap(), file_header());
    blob.free(BlobRef::EMPTY).unwrap();

    // The merged layout survives a reopen.
    let a = blob.write(&[1; 8]).unwrap();
    let b = blob.write(&[2; 8]).unwrap();
    let c = blob.write(&[3; 8]).unwrap();
    blob.write(&[4; 8]).unwrap();
    blob.free(b).unwrap();
    blob.free(a).unwrap();
    let before = blob.stats();
    drop(blob);
    let mut reopened = BlobFile::open_or_create(&path).unwrap();
    assert_eq!(reopened.stats(), before);
    assert_eq!(reopened.read(c).unwrap(), [3; 8]);
    assert_eq!(reopened.write(&[9; 24]).unwrap(), BlobRef::new(24, 24));
}

#[test]
fn reuses_best_fit_block() {
    let directory = tempdir().unwrap();
    let (_path, mut blob) = blob_in(&directory);
    let big = blob.write(&[1; 64]).unwrap(); // header 16, capacity 64
    blob.write(b"sep").unwrap(); // header 88
    let small = blob.write(&[2; 24]).unwrap(); // header 104, capacity 24
    blob.write(b"sep").unwrap(); // header 136
    blob.free(big).unwrap();
    blob.free(small).unwrap();
    let file_len = blob.stats().file_len;

    // 20 bytes (capacity 24): exact fit in the 24-byte block, not the 64.
    let exact = blob.write(&[3; 20]).unwrap();
    assert_eq!(exact, BlobRef::new(112, 20));
    // 40 bytes in the 64-byte block: the rest is 24 >= 16, so it is split
    // into a live 40 and a free block of 24 - 8 = 16.
    let split = blob.write(&[4; 40]).unwrap();
    assert_eq!(split, BlobRef::new(24, 40));
    let stats = blob.stats();
    assert_eq!(stats.free_blocks, 1);
    assert_eq!(stats.largest_free_block, 16);
    // 9 bytes (capacity 16) in the 16-byte rest: rest 0, no split.
    let no_split = blob.write(&[5; 9]).unwrap();
    assert_eq!(no_split, BlobRef::new(72, 9));
    assert_eq!(blob.stats().free_blocks, 0);
    assert_eq!(blob.stats().file_len, file_len, "the file did not grow");

    // A free block with a rest under 16 bytes is used whole.
    blob.free(no_split).unwrap();
    let whole = blob.write(&[6; 1]).unwrap(); // capacity 8 in a 16-byte block
    assert_eq!(whole, BlobRef::new(72, 1));
    assert_eq!(blob.stats().free_blocks, 0);
    assert_eq!(blob.read(exact).unwrap(), [3; 20]);
    assert_eq!(blob.read(split).unwrap(), [4; 40]);
}

#[test]
fn updates_in_place_or_moves() {
    let directory = tempdir().unwrap();
    let (_path, mut blob) = blob_in(&directory);
    let value = blob.write(b"abc").unwrap(); // capacity 8
    blob.write(b"tail").unwrap();

    let shorter = blob.update(value, b"ab").unwrap();
    assert_eq!(shorter, BlobRef::new(value.offset, 2));
    let full = blob.update(shorter, b"12345678").unwrap();
    assert_eq!(full, BlobRef::new(value.offset, 8));
    assert_eq!(blob.read(full).unwrap(), b"12345678");

    let moved = blob.update(full, b"123456789").unwrap();
    assert_ne!(moved.offset, value.offset);
    assert_eq!(blob.read(moved).unwrap(), b"123456789");
    assert!(blob.read(full).is_err(), "the old block is free");
    assert_eq!(blob.stats().free_blocks, 1);

    // Empty <-> value.
    let emptied = blob.update(moved, b"").unwrap();
    assert_eq!(emptied, BlobRef::EMPTY);
    let refilled = blob.update(BlobRef::EMPTY, b"back").unwrap();
    assert_eq!(blob.read(refilled).unwrap(), b"back");
    assert_eq!(refilled.offset, value.offset, "reuses the freed block");
}

#[test]
fn stats_survive_reopen() {
    let directory = tempdir().unwrap();
    let (path, mut blob) = blob_in(&directory);
    let refs: Vec<BlobRef> = [5_usize, 30, 8, 100, 1]
        .iter()
        .map(|len| blob.write(&vec![7; *len]).unwrap())
        .collect();
    blob.free(refs[1]).unwrap();
    blob.free(refs[3]).unwrap();
    let stats = blob.stats();
    assert_eq!(
        stats,
        BlobStats {
            // 16 + (8+8) + (8+32) + (8+8) + (8+104) + (8+8)
            file_len: 216,
            live_blocks: 3,
            live_bytes: 8 + 8 + 8,
            free_blocks: 2,
            free_bytes: (8 + 32) + (8 + 104),
            largest_free_block: 104,
        }
    );
    blob.sync_data().unwrap();
    blob.sync_all().unwrap();
    drop(blob);
    assert_eq!(BlobFile::open_or_create(&path).unwrap().stats(), stats);
}

#[test]
fn refuses_leftover_compaction_and_bad_sizes() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.blob");
    fs::write(compact_path(&path), b"").unwrap();
    let error = open_error(&path);
    assert!(error.contains("values.blob.compact"), "{error}");
    assert!(error.contains("interrupted compaction"), "{error}");

    fs::remove_file(compact_path(&path)).unwrap();
    let mut blob = BlobFile::open_or_create(&path).unwrap();
    // Opening a directory fails cleanly.
    assert!(BlobFile::open_or_create(directory.path()).is_err());
    // A value read into a buffer of the wrong size is rejected.
    let value = blob.write(b"abc").unwrap();
    assert!(blob.read_into(value, &mut [0; 2]).is_err());
}

#[test]
fn freeing_drops_every_free_block_at_the_end() {
    // Two adjacent free blocks, as a crash between a free and its merge can
    // leave them, followed by a live block.
    let directory = tempdir().unwrap();
    let path = directory.path().join("adjacent.blob");
    let mut bytes = file_header();
    bytes.extend(header(false, 8));
    bytes.extend([0; 8]);
    bytes.extend(header(false, 8));
    bytes.extend([0; 8]);
    bytes.extend(header(true, 8));
    bytes.extend(*b"live\0\0\0\0");
    fs::write(&path, &bytes).unwrap();

    let mut blob = BlobFile::open_or_create(&path).unwrap();
    assert_eq!(blob.stats().free_blocks, 2);
    // Freeing the live block merges it with the free block just before it,
    // truncates the file there, and then drops the remaining free block.
    blob.free(BlobRef::new(56, 4)).unwrap();
    assert_eq!(blob.stats().file_len, 16);
    assert_eq!(fs::read(&path).unwrap(), file_header());
}

#[test]
fn blob_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BlobFile>();
    assert_send_sync::<
        rust_serial_file::file::struct_blob_file::StructBlobFile<
            rust_serial_file::serialized_struct::BlobRef,
        >,
    >();
}
