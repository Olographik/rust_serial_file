//! [`Store`] over a `StructFile<Sample>`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rust_serial_file::{
    adapter::{abstract_serial_read::SerialDataReader, abstract_serial_write::SerialDataWriter},
    file::struct_file::StructFile,
    serialized_struct::{EncodeRaw, SerializedStruct},
};

use crate::{
    Profile, Store,
    record::{Checksum, Sample},
};

const FILE_NAME: &str = "samples.bin";

/// `StructFile<Sample>` in `<directory>/samples.bin`, without a metadata
/// prefix.
pub struct SerialStore {
    /// Always `Some` between operations; taken only while a batch is
    /// appended through the raw `FileMetadata` block write.
    file: Option<StructFile<Sample>>,
    path: PathBuf,
    profile: Profile,
}

impl SerialStore {
    fn file(&self) -> &StructFile<Sample> {
        self.file
            .as_ref()
            .expect("the struct file is always present")
    }

    fn file_mut(&mut self) -> &mut StructFile<Sample> {
        self.file
            .as_mut()
            .expect("the struct file is always present")
    }

    fn sync_if_durable(&mut self) -> Result<()> {
        match self.profile {
            Profile::Fast => Ok(()),
            Profile::Durable => self.file_mut().sync_data(),
        }
    }

    fn open_path(path: PathBuf, profile: Profile) -> Result<Self> {
        let file = StructFile::open_or_create(&path, 0)?;
        Ok(Self {
            file: Some(file),
            path,
            profile,
        })
    }

    /// Appends one block of encoded rows with a single positioned write.
    fn append_block(&mut self, samples: &[Sample], buffer: &mut Vec<u8>) -> Result<()> {
        buffer.clear();
        buffer.resize(samples.len() * Sample::SIZE, 0);
        for (sample, row) in samples.iter().zip(buffer.chunks_exact_mut(Sample::SIZE)) {
            sample.encode(row);
        }
        // `StructFile` has no block append, so append through its metadata
        // and wrap it again (one length read per batch).
        let mut metadata = self
            .file
            .take()
            .expect("the struct file is always present")
            .into_file_metadata();
        let result = metadata
            .element_count()
            .and_then(|start| metadata.write_rows(start, buffer));
        self.file = Some(StructFile::new(metadata)?);
        result
    }
}

impl Store for SerialStore {
    const NAME: &'static str = "struct_file";

    fn create(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Self::open_path(path, profile)
    }

    fn open(directory: &Path, profile: Profile) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        anyhow::ensure!(path.exists(), "{} does not exist", path.display());
        Self::open_path(path, profile)
    }

    fn append_one(&mut self, samples: &[Sample]) -> Result<()> {
        let file = self.file_mut();
        for sample in samples {
            file.serial_write(*sample)?;
        }
        self.sync_if_durable()
    }

    fn append_batches(&mut self, samples: &[Sample], batch: usize) -> Result<()> {
        let mut buffer = Vec::new();
        for chunk in samples.chunks(batch.max(1)) {
            self.append_block(chunk, &mut buffer)?;
            self.sync_if_durable()?;
        }
        Ok(())
    }

    fn len(&self) -> Result<u64> {
        Ok(self.file().serial_len() as u64)
    }

    fn scan(&self) -> Result<Checksum> {
        let mut checksum = Checksum::new();
        for sample in self.file().serial_iter()? {
            checksum.add(&sample?);
        }
        Ok(checksum)
    }

    fn get(&self, index: u64, _time: u64) -> Result<Sample> {
        self.file().read_value_at(usize::try_from(index)?)
    }

    fn find(&self, time: u64) -> Result<Option<Sample>> {
        let file = self.file();
        file.serial_find_by(&time)?
            .map(|index| file.read_value_at(index))
            .transpose()
    }

    fn range(&self, start_time: u64, count: usize, out: &mut Vec<Sample>) -> Result<()> {
        let file = self.file();
        let start = file
            .serial_find_by(&start_time)?
            .context("range start time not found")?;
        let count = count.min(file.serial_len() - start);
        let mut rows = file
            .file_metadata()
            .iter_rows_range(start as u64, count as u64)?;
        let mut row = [0; Sample::SIZE];
        while rows.next_into(&mut row)? {
            out.push(Sample::try_decode(&row)?);
        }
        Ok(())
    }

    fn update(&mut self, updates: &[(u64, Sample)]) -> Result<()> {
        let file = self.file_mut();
        for (index, sample) in updates {
            file.write_value_at(usize::try_from(*index)?, sample)?;
        }
        self.sync_if_durable()
    }

    fn insert(&mut self, inserts: &[(u64, Sample)]) -> Result<()> {
        let file = self.file_mut();
        for (index, sample) in inserts {
            file.serial_insert(usize::try_from(*index)?, *sample)?;
        }
        self.sync_if_durable()
    }

    fn remove(&mut self, removals: &[(u64, u64)]) -> Result<()> {
        let file = self.file_mut();
        for (index, _time) in removals {
            file.serial_remove(usize::try_from(*index)?)?;
        }
        self.sync_if_durable()
    }

    fn disk_size(&self) -> Result<u64> {
        Ok(fs::metadata(&self.path)?.len())
    }
}
