//! [`Store`] over a SQLite table, through `rusqlite` (bundled SQLite).

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    Profile, Store,
    record::{Checksum, Sample},
};

const FILE_NAME: &str = "samples.db";
const COLUMNS: &str = "time, sensor, flags, value";

/// The `sample` table in `<directory>/samples.db`, keyed by `time`.
pub struct SqliteStore {
    connection: Connection,
    path: PathBuf,
}

/// Opens a connection with the pragmas of `profile`:
/// `journal_mode=WAL` and `synchronous` from [`Profile::sqlite_synchronous`].
///
/// # Errors
///
/// Fails if the database cannot be opened or a pragma cannot be set.
pub fn connect(path: &Path, profile: Profile) -> Result<Connection> {
    let connection =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    anyhow::ensure!(mode.eq_ignore_ascii_case("wal"), "WAL mode refused: {mode}");
    connection.pragma_update(None, "synchronous", profile.sqlite_synchronous())?;
    Ok(connection)
}

fn to_sql_time(time: u64) -> Result<i64> {
    i64::try_from(time).context("time does not fit in a SQLite INTEGER")
}

fn sample_from_row(row: &Row<'_>) -> rusqlite::Result<Sample> {
    Ok(Sample {
        time: row.get::<_, i64>(0)? as u64,
        sensor: row.get(1)?,
        flags: row.get(2)?,
        value: row.get(3)?,
    })
}

impl SqliteStore {
    fn open_path(path: PathBuf, profile: Profile) -> Result<Self> {
        let connection = connect(&path, profile)?;
        Ok(Self { connection, path })
    }

    /// Runs `insert`-like statements for `samples` in one transaction.
    fn write_in_transaction(&mut self, sql: &str, samples: &[Sample]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(sql)?;
            for sample in samples {
                statement.execute(params![
                    to_sql_time(sample.time)?,
                    sample.sensor,
                    sample.flags,
                    sample.value
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

const INSERT: &str = "INSERT INTO sample (time, sensor, flags, value) VALUES (?1, ?2, ?3, ?4)";

impl Store for SqliteStore {
    const NAME: &'static str = "sqlite";

    fn create(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", path.display()));
            if file.exists() {
                fs::remove_file(file)?;
            }
        }
        let store = Self::open_path(path, profile)?;
        store.connection.execute(
            "CREATE TABLE sample (\
                time INTEGER PRIMARY KEY, \
                sensor INTEGER NOT NULL, \
                flags INTEGER NOT NULL, \
                value REAL NOT NULL\
            ) WITHOUT ROWID",
            [],
        )?;
        Ok(store)
    }

    fn open(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        anyhow::ensure!(path.exists(), "{} does not exist", path.display());
        Self::open_path(path, profile)
    }

    fn append_one(&mut self, samples: &[Sample]) -> Result<()> {
        self.write_in_transaction(INSERT, samples)
    }

    fn append_batches(&mut self, samples: &[Sample], batch: usize) -> Result<()> {
        for chunk in samples.chunks(batch.max(1)) {
            self.write_in_transaction(INSERT, chunk)?;
        }
        Ok(())
    }

    fn len(&self) -> Result<u64> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM sample", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    fn scan(&self) -> Result<Checksum> {
        let mut statement = self
            .connection
            .prepare_cached(&format!("SELECT {COLUMNS} FROM sample ORDER BY time"))?;
        let mut checksum = Checksum::new();
        for sample in statement.query_map([], sample_from_row)? {
            checksum.add(&sample?);
        }
        Ok(checksum)
    }

    fn get(&self, _index: u64, time: u64) -> Result<Sample> {
        self.find(time)?
            .with_context(|| format!("no sample with time {time}"))
    }

    fn find(&self, time: u64) -> Result<Option<Sample>> {
        let mut statement = self
            .connection
            .prepare_cached(&format!("SELECT {COLUMNS} FROM sample WHERE time = ?1"))?;
        Ok(statement
            .query_row([to_sql_time(time)?], sample_from_row)
            .optional()?)
    }

    fn range(&self, start_time: u64, count: usize, out: &mut Vec<Sample>) -> Result<()> {
        let mut statement = self.connection.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM sample WHERE time >= ?1 ORDER BY time LIMIT ?2"
        ))?;
        for sample in statement.query_map(
            params![to_sql_time(start_time)?, i64::try_from(count)?],
            sample_from_row,
        )? {
            out.push(sample?);
        }
        Ok(())
    }

    fn update(&mut self, updates: &[(u64, Sample)]) -> Result<()> {
        let samples: Vec<Sample> = updates.iter().map(|(_, sample)| *sample).collect();
        self.write_in_transaction(
            "UPDATE sample SET sensor = ?2, flags = ?3, value = ?4 WHERE time = ?1",
            &samples,
        )
    }

    fn insert(&mut self, inserts: &[(u64, Sample)]) -> Result<()> {
        let samples: Vec<Sample> = inserts.iter().map(|(_, sample)| *sample).collect();
        self.write_in_transaction(INSERT, &samples)
    }

    fn remove(&mut self, removals: &[(u64, u64)]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached("DELETE FROM sample WHERE time = ?1")?;
            for (_index, time) in removals {
                statement.execute([to_sql_time(*time)?])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// The database file plus its WAL file (the `-shm` index is excluded:
    /// it is a shared-memory file, not data).
    fn disk_size(&self) -> Result<u64> {
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        let wal_len = fs::metadata(&wal)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        Ok(fs::metadata(&self.path)?.len() + wal_len)
    }
}

/// Reads a pragma's current value as text, for checks and the report.
///
/// # Errors
///
/// Fails if the pragma cannot be read.
pub fn pragma(connection: &Connection, name: &str) -> Result<String> {
    let value: rusqlite::types::Value =
        connection.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))?;
    Ok(match value {
        rusqlite::types::Value::Integer(value) => value.to_string(),
        rusqlite::types::Value::Text(value) => value,
        other => format!("{other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use crate::Profile;

    use super::{connect, pragma};

    #[test]
    fn profiles_set_pragmas() {
        let directory = tempfile::tempdir().unwrap();
        for (profile, synchronous) in [(Profile::Fast, "0"), (Profile::Durable, "2")] {
            let connection = connect(
                &directory.path().join(format!("{}.db", profile.name())),
                profile,
            )
            .unwrap();
            assert_eq!(pragma(&connection, "journal_mode").unwrap(), "wal");
            // SQLite reports synchronous as a number: OFF = 0, FULL = 2.
            assert_eq!(pragma(&connection, "synchronous").unwrap(), synchronous);
        }
    }
}
