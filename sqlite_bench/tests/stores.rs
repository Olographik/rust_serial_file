use std::path::Path;

use rusqlite::Connection;
use sqlite_bench::{
    Profile, Store, check_same_data,
    named_store::{BlobNamedStore, NamedStore, SqliteNamedStore},
    record::{Sample, checksum_of, generate, generate_names, missing_time_after},
    serial_store::SerialStore,
    sqlite_store::SqliteStore,
};
use tempfile::tempdir;

const COUNT: usize = 1_000;

fn loaded<S: Store>(directory: &Path, profile: Profile, samples: &[Sample]) -> S {
    let mut store = S::create(directory, profile).unwrap();
    store.append_batches(samples, 300).unwrap();
    store
}

#[test]
fn stores_hold_same_data() {
    let samples = generate(10_000, 1);
    let directory = tempdir().unwrap();
    let serial: SerialStore = loaded(directory.path(), Profile::Fast, &samples);
    let sqlite: SqliteStore = loaded(directory.path(), Profile::Fast, &samples);
    check_same_data(&serial, &sqlite).unwrap();
    assert_eq!(serial.scan().unwrap(), checksum_of(&samples));
    assert_eq!(serial.len().unwrap(), 10_000);

    // Appending one by one gives the same data as batches.
    let other = tempdir().unwrap();
    let mut one_by_one = SerialStore::create(other.path(), Profile::Durable).unwrap();
    one_by_one.append_one(&samples).unwrap();
    let mut sqlite_one = SqliteStore::create(other.path(), Profile::Durable).unwrap();
    sqlite_one.append_one(&samples).unwrap();
    check_same_data(&one_by_one, &serial).unwrap();
    check_same_data(&sqlite_one, &sqlite).unwrap();

    // Reopening keeps the data.
    drop((serial, sqlite));
    let serial = SerialStore::open(directory.path(), Profile::Fast).unwrap();
    let sqlite = SqliteStore::open(directory.path(), Profile::Fast).unwrap();
    check_same_data(&serial, &sqlite).unwrap();
    assert!(serial.disk_size().unwrap() >= 10_000 * 24);
    assert!(sqlite.disk_size().unwrap() > 0);
    assert!(!serial.is_empty().unwrap());
}

#[test]
fn detects_mismatch() {
    let samples = generate(COUNT, 2);
    let directory = tempdir().unwrap();
    let serial: SerialStore = loaded(directory.path(), Profile::Fast, &samples);
    let sqlite: SqliteStore = loaded(directory.path(), Profile::Fast, &samples);
    check_same_data(&serial, &sqlite).unwrap();

    let connection = Connection::open(directory.path().join("samples.db")).unwrap();
    connection
        .execute(
            "UPDATE sample SET value = value + 1 WHERE time = ?1",
            [samples[500].time as i64],
        )
        .unwrap();
    let error = check_same_data(&serial, &sqlite).unwrap_err().to_string();
    assert!(error.contains("hold different data"), "{error}");

    assert!(SerialStore::open(&directory.path().join("missing"), Profile::Fast).is_err());
    assert!(SqliteStore::open(&directory.path().join("missing"), Profile::Fast).is_err());
}

/// Runs every benchmarked operation once on `store` and returns what can be
/// observed, to compare the two storages.
fn exercise<S: Store>(store: &mut S, samples: &[Sample]) -> Vec<String> {
    let mut seen = Vec::new();
    for index in [0, 1, 499, 999] {
        let sample = store.get(index as u64, samples[index].time).unwrap();
        seen.push(format!("get {index}: {sample:?}"));
    }
    for index in [0, 500, 999] {
        seen.push(format!(
            "find {:?}",
            store.find(samples[index].time).unwrap()
        ));
        seen.push(format!(
            "miss {:?}",
            store.find(missing_time_after(&samples[index])).unwrap()
        ));
    }
    let mut range = Vec::new();
    store.range(samples[990].time, 100, &mut range).unwrap();
    seen.push(format!("range {} {:?}", range.len(), range.first()));
    range.clear();
    store.range(samples[10].time, 100, &mut range).unwrap();
    seen.push(format!(
        "range {} {:?} {:?}",
        range.len(),
        range.first(),
        range.last()
    ));

    let updates: Vec<(u64, Sample)> = [3_usize, 700]
        .iter()
        .map(|&index| {
            let mut sample = samples[index];
            sample.value = -1.0;
            sample.flags = 7;
            (index as u64, sample)
        })
        .collect();
    store.update(&updates).unwrap();
    seen.push(format!(
        "after update {:?}",
        store.find(samples[700].time).unwrap()
    ));

    // Insert after rows 800 and 100 (descending), then remove them again.
    let inserts: Vec<(u64, Sample)> = [800_usize, 100]
        .iter()
        .map(|&index| {
            let mut sample = samples[index];
            sample.time = missing_time_after(&samples[index]);
            (index as u64 + 1, sample)
        })
        .collect();
    store.insert(&inserts).unwrap();
    seen.push(format!("len after insert {}", store.len().unwrap()));
    seen.push(format!(
        "inserted {:?}",
        store.get(101, inserts[1].1.time).unwrap()
    ));
    seen.push(format!("scan {:?}", store.scan().unwrap()));
    let removals: Vec<(u64, u64)> = inserts
        .iter()
        .enumerate()
        .map(|(position, (index, sample))| {
            // The first insert (at 801) was shifted by the second one.
            let shift = u64::from(position == 0);
            (index + shift, sample.time)
        })
        .collect();
    store.remove(&removals).unwrap();
    seen.push(format!("len after remove {}", store.len().unwrap()));
    seen.push(format!("scan {:?}", store.scan().unwrap()));
    seen
}

#[test]
fn operations_agree() {
    let samples = generate(COUNT, 3);
    for profile in Profile::ALL {
        let directory = tempdir().unwrap();
        let mut serial: SerialStore = loaded(directory.path(), profile, &samples);
        let mut sqlite: SqliteStore = loaded(directory.path(), profile, &samples);
        let serial_seen = exercise(&mut serial, &samples);
        let sqlite_seen = exercise(&mut sqlite, &samples);
        assert_eq!(serial_seen, sqlite_seen);
        // The checks are meaningful: lookups found values, misses did not.
        assert!(serial_seen.iter().any(|line| line.starts_with("find Some")));
        assert!(
            serial_seen
                .iter()
                .all(|line| !line.starts_with("miss Some"))
        );
        // A range starting at row 990 is cut at the end (10 rows); one at
        // row 10 is full (100 rows).
        assert!(serial_seen.iter().any(|line| line.starts_with("range 10 ")));
        assert!(
            serial_seen
                .iter()
                .any(|line| line.starts_with("range 100 "))
        );
        assert!(serial_seen.contains(&format!("len after insert {}", COUNT + 2)));
        assert!(serial_seen.contains(&format!("len after remove {COUNT}")));
        check_same_data(&serial, &sqlite).unwrap();
    }
}

#[test]
fn named_stores_agree() {
    let names = generate_names(2_000, 4);
    for profile in Profile::ALL {
        let directory = tempdir().unwrap();
        let mut blob = BlobNamedStore::create(directory.path(), profile).unwrap();
        let mut sqlite = SqliteNamedStore::create(directory.path(), profile).unwrap();
        blob.append_batches(&names[..1_500], 400).unwrap();
        blob.append_batches(&names[1_500..], 400).unwrap();
        sqlite.append_batches(&names[..1_500], 400).unwrap();
        sqlite.append_batches(&names[1_500..], 400).unwrap();
        assert_eq!(blob.scan().unwrap(), sqlite.scan().unwrap());
        assert_eq!(blob.scan().unwrap().count(), 2_000);

        let updates: Vec<(usize, String)> = (0..2_000)
            .step_by(7)
            .map(|index| (index, format!("{}-renamed-longer", names[index])))
            .collect();
        blob.update(&updates).unwrap();
        sqlite.update(&updates).unwrap();
        assert_eq!(blob.scan().unwrap(), sqlite.scan().unwrap());
        assert!(blob.disk_size().unwrap() > 0);
        assert!(sqlite.disk_size().unwrap() > 0);
        assert!(sqlite.update(&[(5_000, "x".to_owned())]).is_err());
    }
}
