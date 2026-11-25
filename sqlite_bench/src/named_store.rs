//! Records with a variable-length name: `StructBlobFile` (rows with a
//! `BlobRef` plus a blob file) against a SQLite `TEXT` column.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use rust_serial_file::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    file::{
        blob_file::{BlobFields, BlobRef},
        struct_blob_file::StructBlobFile,
    },
    serialized_struct,
};
use serialized_struct_derive::SerializedStruct;

use crate::{Profile, record::Checksum, sqlite_store::connect};

/// A row of the struct file: a time and a reference to its name.
#[derive(SerializedStruct, Debug, Clone, Copy, PartialEq)]
pub struct NamedRow {
    /// Key, `index * 16`.
    pub time: u64,
    /// The name, in the blob file.
    pub name: BlobRef,
}

impl BlobFields for NamedRow {
    fn blob_refs_mut(&mut self, f: &mut dyn FnMut(&mut BlobRef)) {
        f(&mut self.name);
    }
}

/// Time of the record at `index`.
pub fn named_time(index: usize) -> u64 {
    index as u64 * 16
}

/// A storage of `(time, name)` records.
pub trait NamedStore: Sized {
    /// Name used in benchmark ids.
    const NAME: &'static str;

    /// Creates an empty storage in `directory`.
    fn create(directory: &Path, profile: Profile) -> Result<Self>;

    /// Opens the storage previously created in `directory`.
    fn open(directory: &Path, profile: Profile) -> Result<Self>;

    /// Appends `names` (record `i` gets time [`named_time`]`(len + i)`) in
    /// batches of `batch`, each batch durable under [`Profile::Durable`].
    fn append_batches(&mut self, names: &[String], batch: usize) -> Result<()>;

    /// Reads every record in order and returns their checksum
    /// ([`Checksum::add_named`]).
    fn scan(&self) -> Result<Checksum>;

    /// Replaces names: `(index, new name)`.
    fn update(&mut self, updates: &[(usize, String)]) -> Result<()>;

    /// Bytes used on disk.
    fn disk_size(&self) -> Result<u64>;
}

/// `StructBlobFile<NamedRow>` in `<directory>/named.bin` and `named.blob`.
pub struct BlobNamedStore {
    file: StructBlobFile<NamedRow>,
    paths: [PathBuf; 2],
    profile: Profile,
}

impl BlobNamedStore {
    fn sync_if_durable(&mut self) -> Result<()> {
        match self.profile {
            Profile::Fast => Ok(()),
            Profile::Durable => self.file.sync_data(),
        }
    }
}

impl NamedStore for BlobNamedStore {
    const NAME: &'static str = "struct_blob_file";

    fn create(directory: &Path, profile: Profile) -> Result<Self> {
        let paths = [directory.join("named.bin"), directory.join("named.blob")];
        for path in &paths {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Self::open(directory, profile)
    }

    fn open(directory: &Path, profile: Profile) -> Result<Self> {
        let paths = [directory.join("named.bin"), directory.join("named.blob")];
        let file = StructBlobFile::open_or_create(&paths[0], &paths[1], 0)?;
        Ok(Self {
            file,
            paths,
            profile,
        })
    }

    fn append_batches(&mut self, names: &[String], batch: usize) -> Result<()> {
        for chunk in names.chunks(batch.max(1)) {
            {
                let (mut rows, blob) = self.file.parts_mut()?;
                for name in chunk {
                    let time = named_time(rows.serial_len());
                    let name = blob.write_str(name)?;
                    rows.serial_write(NamedRow { time, name })?;
                }
            }
            self.sync_if_durable()?;
        }
        Ok(())
    }

    fn scan(&self) -> Result<Checksum> {
        let (rows, blob) = self.file.parts()?;
        let mut checksum = Checksum::new();
        for row in rows.serial_iter()? {
            let row = row?;
            checksum.add_named(row.time, &blob.read_string(row.name)?);
        }
        Ok(checksum)
    }

    fn update(&mut self, updates: &[(usize, String)]) -> Result<()> {
        {
            let (mut rows, blob) = self.file.parts_mut()?;
            for (index, name) in updates {
                let mut row = rows.read_value_at(*index)?;
                row.name = blob.update(row.name, name.as_bytes())?;
                rows.write_value_at(*index, &row)?;
            }
        }
        self.sync_if_durable()
    }

    fn disk_size(&self) -> Result<u64> {
        let mut total = 0;
        for path in &self.paths {
            total += fs::metadata(path)?.len();
        }
        Ok(total)
    }
}

/// The `named` table in `<directory>/named.db`.
pub struct SqliteNamedStore {
    connection: Connection,
    path: PathBuf,
}

impl SqliteNamedStore {
    fn open_path(path: PathBuf, profile: Profile) -> Result<Self> {
        let connection = connect(&path, profile)?;
        Ok(Self { connection, path })
    }
}

impl NamedStore for SqliteNamedStore {
    const NAME: &'static str = "sqlite";

    fn create(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join("named.db");
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", path.display()));
            if file.exists() {
                fs::remove_file(file)?;
            }
        }
        let store = Self::open_path(path, profile)?;
        store.connection.execute(
            "CREATE TABLE named (time INTEGER PRIMARY KEY, name TEXT NOT NULL) WITHOUT ROWID",
            [],
        )?;
        Ok(store)
    }

    fn open(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join("named.db");
        anyhow::ensure!(path.exists(), "{} does not exist", path.display());
        Self::open_path(path, profile)
    }

    fn append_batches(&mut self, names: &[String], batch: usize) -> Result<()> {
        let mut next: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM named", [], |row| row.get(0))?;
        for chunk in names.chunks(batch.max(1)) {
            let transaction = self.connection.transaction()?;
            {
                let mut statement =
                    transaction.prepare_cached("INSERT INTO named (time, name) VALUES (?1, ?2)")?;
                for name in chunk {
                    statement.execute(params![named_time(next as usize) as i64, name])?;
                    next += 1;
                }
            }
            transaction.commit()?;
        }
        Ok(())
    }

    fn scan(&self) -> Result<Checksum> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT time, name FROM named ORDER BY time")?;
        let mut checksum = Checksum::new();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let time: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            checksum.add_named(time as u64, &name);
        }
        Ok(checksum)
    }

    fn update(&mut self, updates: &[(usize, String)]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut statement =
                transaction.prepare_cached("UPDATE named SET name = ?2 WHERE time = ?1")?;
            for (index, name) in updates {
                let changed = statement.execute(params![named_time(*index) as i64, name])?;
                anyhow::ensure!(changed == 1, "no record at index {index}");
            }
        }
        transaction
            .commit()
            .context("failed to commit the name updates")
    }

    fn disk_size(&self) -> Result<u64> {
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        let wal_len = fs::metadata(&wal)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        Ok(fs::metadata(&self.path)?.len() + wal_len)
    }
}
