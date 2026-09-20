use anyhow::{Ok, anyhow};
use std::{
    cmp::max,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    mem::size_of,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

pub mod position;
pub mod struct_file;

pub trait LenSized {
    fn len(&self) -> u64;
}

impl LenSized for Arc<File> {
    fn len(&self) -> u64 {
        match self.metadata() {
            std::result::Result::Ok(m) => m.len(),
            std::result::Result::Err(_) => todo!(),
        }
    }
}

pub trait SeekRead {
    fn seek_read(&mut self, pos: SeekFrom, buf: &mut [u8]) -> anyhow::Result<(u64, usize)>;
}
pub trait SeekWrite {
    fn seek_write(&mut self, pos: SeekFrom, buf: &[u8]) -> anyhow::Result<(u64, usize)>;
}

impl<T: Read + Seek> SeekRead for T {
    fn seek_read(&mut self, pos: SeekFrom, buf: &mut [u8]) -> anyhow::Result<(u64, usize)> {
        let pos = self.seek(pos)?;
        let size = self.read(buf)?;
        Ok((pos, size))
    }
}

impl<T: Write + Seek> SeekWrite for T {
    fn seek_write(&mut self, pos: SeekFrom, buf: &[u8]) -> anyhow::Result<(u64, usize)> {
        let pos = self.seek(pos)?;
        let size = self.write(buf)?;
        Ok((pos, size))
    }
}

pub trait FileNeededTrait: SeekRead + SeekWrite + Write + LenSized + Clone {}

impl<T> FileNeededTrait for T where T: SeekRead + SeekWrite + Write + LenSized + Clone {}

/// Read write lock not available as the [`File::read`] and [`File::seek`] function
/// take a mutable file as input -> ie cannot ensure the File position will be ok
/// when reading.
/// Thus the mutex will block weither we are in write or read mode
#[derive(Clone, Debug)]
pub struct ProtectedFile(Arc<Mutex<File>>);

impl ProtectedFile {
    pub fn new(file: File) -> Self {
        Self(Arc::new(Mutex::new(file)))
    }

    pub fn lock(&self) -> anyhow::Result<MutexGuard<'_, File>> {
        self.0.lock().map_err(|_| anyhow!("Poisoned lock"))
    }

    pub fn lock_io(&self) -> std::io::Result<MutexGuard<'_, File>> {
        self.0.lock().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                anyhow::Error::msg("Poisoned lock"),
            )
        })
    }
}

impl SeekRead for ProtectedFile {
    fn seek_read(&mut self, pos: SeekFrom, buf: &mut [u8]) -> anyhow::Result<(u64, usize)> {
        let mut lock = self.lock()?;
        let pos = lock.seek(pos)?;
        let size = lock.read(buf)?;
        Ok((pos, size))
    }
}

impl SeekWrite for ProtectedFile {
    fn seek_write(&mut self, pos: SeekFrom, buf: &[u8]) -> anyhow::Result<(u64, usize)> {
        let mut lock = self.lock()?;
        let pos = lock.seek(pos)?;
        let size = lock.write(buf)?;
        Ok((pos, size))
    }
}

impl Write for ProtectedFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut lock = self.lock_io()?;
        lock.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut lock = self.lock_io()?;
        lock.flush()
    }
}

impl LenSized for ProtectedFile {
    fn len(&self) -> u64 {
        let locked = if let std::result::Result::Ok(l) = self.lock() {
            l.metadata()
        } else {
            return 0;
        };
        match locked {
            std::result::Result::Ok(m) => m.len(),
            std::result::Result::Err(_) => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileReference<T>
where
    T: FileNeededTrait,
{
    pub file: T,
    /// Has to be fix for a given file !
    /// The size of a row or what can be considered as row
    pub size_of_a_row: usize,
    /// Has to be fix for a given file !
    /// Offset at start of the file to contains data (like metadata)
    pub offset: u64,
}

impl FileReference<ProtectedFile> {
    pub fn from_path(
        file: impl Into<PathBuf>,
        size_of_a_row: usize,
        offset: u64,
        read_only: bool,
    ) -> anyhow::Result<Self> {
        let f = File::options()
            .read(true)
            .write(!read_only)
            .create(!read_only)
            .open(file.into())?;
        Ok(Self::new(ProtectedFile::new(f), size_of_a_row, offset))
    }

    pub fn read_only(
        file: impl Into<PathBuf>,
        size_of_a_row: usize,
        offset: u64,
    ) -> anyhow::Result<Self> {
        Self::from_path(file, size_of_a_row, offset, true)
    }

    pub fn new_protected(file: File, size_of_a_row: usize, offset: u64) -> Self {
        FileReference::new(ProtectedFile::new(file), size_of_a_row, offset)
    }
}

impl FileReference<Arc<File>> {
    pub fn free_from_path(
        file: impl Into<PathBuf>,
        size_of_a_row: usize,
        offset: u64,
        read_only: bool,
    ) -> anyhow::Result<Self> {
        let f = File::options()
            .read(true)
            .write(!read_only)
            .create(!read_only)
            .open(file.into())?;
        Ok(Self::new(Arc::new(f), size_of_a_row, offset))
    }

    pub fn free_read_only(
        file: impl Into<PathBuf>,
        size_of_a_row: usize,
        offset: u64,
    ) -> anyhow::Result<Self> {
        Self::free_from_path(file, size_of_a_row, offset, true)
    }

    pub fn new_free(file: File, size_of_a_row: usize, offset: u64) -> Self {
        FileReference::new(Arc::new(file), size_of_a_row, offset)
    }
}

impl<T> FileReference<T>
where
    T: FileNeededTrait,
{
    pub fn new(file: impl Into<T>, size_of_a_row: usize, offset: u64) -> Self {
        Self {
            file: file.into(),
            size_of_a_row: max(size_of_a_row, 1),
            offset,
        }
    }

    pub fn of_with_offset<S: Sized>(file: T, offset: u64) -> Self {
        Self {
            file,
            size_of_a_row: size_of::<S>(),
            offset,
        }
    }
    pub fn of<S: Sized>(file: T) -> Self {
        FileReference::of_with_offset::<S>(file, 0)
    }

    pub fn file_size(&self) -> u64 {
        self.file.len()
    }

    pub fn row_count(&self) -> usize {
        self.to_row_index(self.file_size())
    }

    pub fn to_row_index(&self, file_position: u64) -> usize {
        ((file_position - self.offset as u64) / (self.size_of_a_row as u64)) as usize
    }

    pub fn to_file_row_count(&self, rows: u64) -> u64 {
        rows * self.size_of_a_row as u64
    }

    pub fn to_file_position(&self, row_index: usize) -> u64 {
        self.to_file_row_count(row_index as u64) + self.offset
    }

    pub fn switch_metadata(&self, size_of_a_row: usize, offset: u64) -> Self {
        Self {
            file: self.file.clone(),
            size_of_a_row: size_of_a_row,
            offset: offset,
        }
    }

    pub fn switch_metadata_inplace(&mut self, size_of_a_row: usize, offset: u64) -> &mut Self {
        self.size_of_a_row = size_of_a_row;
        self.offset = offset;

        self
    }

    pub fn switch_file(&self, file: impl Into<T>) -> Self {
        Self::new(file, self.size_of_a_row, self.offset)
    }

    pub fn switch_file_inplace(&mut self, file: impl Into<T>) -> &mut Self {
        self.file = file.into();
        self
    }

    pub fn reader(&self, row_count: usize) -> BufferedReader<T> {
        BufferedReader::wrap(&self, row_count)
    }

    pub fn writer(&self, row_count: usize) -> BufferedWriter<T> {
        BufferedWriter::wrap(&self, row_count)
    }
}

impl<T> Deref for FileReference<T>
where
    T: FileNeededTrait,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        return &self.file;
    }
}
impl<T> DerefMut for FileReference<T>
where
    T: FileNeededTrait,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        return &mut self.file;
    }
}

pub struct BufferedReader<T: FileNeededTrait> {
    file: FileReference<T>,
    // state variable
    buffer: Vec<u8>,
    buffer_position: u64,
    buffer_read_size: usize,
}

impl<T: FileNeededTrait> BufferedReader<T> {
    pub fn wrap(file: &FileReference<T>, row_count: usize) -> Self {
        let size_of_a_row = file.size_of_a_row;
        let capacity = size_of_a_row * row_count;
        Self {
            file: file.clone(),
            buffer: vec![0u8; capacity],
            buffer_position: 0,
            buffer_read_size: 0,
        }
    }

    pub fn file_size(&self) -> u64 {
        self.file.file_size()
    }

    pub fn row_count(&self) -> usize {
        self.file.row_count()
    }

    pub fn reset(&mut self) {
        self.buffer_position = 0;
        self.buffer_read_size = 0;
    }

    // The position is not aligned, will align inside the function
    pub fn reallocate_buffer(&mut self, from_position: u64) -> anyhow::Result<()> {
        let position_without_offset = from_position - self.file.offset;
        let page = position_without_offset / (self.file.size_of_a_row as u64);

        let final_pos = self.file.offset + page * (self.file.size_of_a_row as u64);

        let pair = self
            .file
            .seek_read(SeekFrom::Start(final_pos), &mut self.buffer)?;

        self.buffer_position = pair.0;
        self.buffer_read_size = pair.1;

        Ok(())
    }

    pub fn ensure_buffer(&mut self, file_position: u64) -> anyhow::Result<()> {
        if self.buffer_read_size == 0 {
            self.reallocate_buffer(file_position)?;
        } else if file_position < self.buffer_position {
            self.reallocate_buffer(file_position)?;
        } else {
            let capacity: usize = self.buffer.len();
            let end = self.buffer_position + capacity as u64;
            if file_position >= end {
                self.reallocate_buffer(file_position)?;
            }
        }
        Ok(())
    }

    pub fn read_at_preallocated_file_position(&self, file_position: u64) -> &[u8] {
        let start_of_buffer = (file_position - self.buffer_position) as usize;
        let end = start_of_buffer + self.file.size_of_a_row;
        &self.buffer[start_of_buffer..end]
    }
    pub fn read_at_preallocated(&self, index: usize) -> &[u8] {
        self.read_at_preallocated_file_position(self.file.to_file_position(index))
    }

    pub fn read_at_file_position(&mut self, file_position: u64) -> anyhow::Result<&[u8]> {
        self.ensure_buffer(file_position)?;

        Ok(self.read_at_preallocated_file_position(file_position))
    }

    pub fn read_at(&mut self, index: usize) -> anyhow::Result<&[u8]> {
        self.read_at_file_position(self.file.to_file_position(index))
    }

    pub fn seek(&mut self, index: usize) -> anyhow::Result<()> {
        self.ensure_buffer(self.file.to_file_position(index))
    }
}

pub struct BufferedWriter<T: FileNeededTrait> {
    file: FileReference<T>,
    // state variable
    buffer: Vec<u8>,
    buffer_position: usize,
}

impl<T: FileNeededTrait> BufferedWriter<T> {
    pub fn wrap(file: &FileReference<T>, row_count: usize) -> Self {
        let size_of_a_row = file.size_of_a_row;
        let capacity = size_of_a_row * row_count;
        Self {
            file: file.clone(),
            buffer: Vec::with_capacity(capacity),
            buffer_position: 0,
        }
    }

    pub fn file_size(&mut self) -> u64 {
        self.file.file_size()
    }

    pub fn row_count(&self) -> usize {
        self.file.row_count()
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let file_position = self.file.to_file_position(self.buffer_position);
        let data: &[u8] = &self.buffer;

        Self::write_to_file(&mut self.file, &data, SeekFrom::Start(file_position))?;

        self.buffer_position += self.buffer.len() / self.file.size_of_a_row;
        self.buffer.clear();
        Ok(())
    }

    fn check_flush(&mut self) -> anyhow::Result<()> {
        if self.buffer.len() == self.buffer.capacity() {
            self.flush()?;
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.buffer_position = 0;
        self.buffer.clear();
    }

    pub fn seek(&mut self, position: usize) -> anyhow::Result<()> {
        self.flush()?;
        self.buffer.clear();
        self.buffer_position = position;

        Ok(())
    }

    /// Expect the row to be size of a row, no check done
    pub fn write(&mut self, row: &[u8]) -> anyhow::Result<()> {
        self.check_flush()?;
        self.buffer.extend_from_slice(row);

        Ok(())
    }

    pub fn write_at(&mut self, position: usize, row: &[u8]) -> anyhow::Result<()> {
        self.seek(position)?;
        self.buffer.extend_from_slice(row);
        Ok(())
    }

    pub fn write_many(&mut self, mut rows: &[u8]) -> anyhow::Result<()> {
        let mut row_to_write = rows.len() / self.file.size_of_a_row;

        while row_to_write > 0 {
            let mut space_left = self.buffer.capacity() - self.buffer.len();
            if space_left == 0 {
                self.flush()?;
            }
            space_left *= self.file.size_of_a_row;
            if space_left > rows.len() {
                self.buffer.extend_from_slice(rows);
                row_to_write = 0;
            } else {
                self.buffer.extend_from_slice(&rows[0..space_left]);

                rows = &rows[space_left..];

                row_to_write = rows.len() / self.file.size_of_a_row;
            }
        }
        Ok(())
    }

    pub fn raw_write(&mut self, data: &[u8], file_position: SeekFrom) -> anyhow::Result<()> {
        Self::write_to_file(&mut self.file, data, file_position)
    }
    fn write_to_file(file: &mut T, data: &[u8], file_position: SeekFrom) -> anyhow::Result<()> {
        file.seek_write(file_position, data)?;
        Ok(())
    }
}

impl<T: FileNeededTrait> Drop for BufferedWriter<T> {
    fn drop(&mut self) {
        if let Err(_) = self.flush() {
            // Should do something ?
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        fs::{self, File},
        io::{Read, Write},
        str::from_utf8,
    };

    use crate::file::{BufferedReader, FileReference};

    use super::BufferedWriter;

    #[test]
    fn test_read_buffer() {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_str = "Hello world";

        {
            let mut write_file = File::options()
                .write(true)
                .create(true)
                .open(file_path.clone())
                .unwrap();
            //create a temp file
            write_file.write_all(expected_str.as_bytes()).unwrap();
        }

        let raw_file = File::options().read(true).open(file_path.clone()).unwrap();
        let base_file_ref = FileReference::new_protected(raw_file, expected_str.len(), 0);
        println!("Whole word is a row");
        {
            let mut reader = BufferedReader::wrap(&base_file_ref, 1);

            let row = reader.read_at(0).unwrap();

            let read = from_utf8(row).unwrap();

            println!("Read: {}", read);

            assert_eq!(expected_str.as_bytes(), row);
        }
        println!("a character is a row");
        {
            let file = base_file_ref.switch_metadata(1, 0);
            let mut reader = BufferedReader::wrap(&file, expected_str.len());

            for i in 0..expected_str.len() {
                let row = reader.read_at(i).unwrap();
                let part = from_utf8(row).unwrap();

                println!("Read at [{}]: {}", i, part);
                let to_compare = expected_str.as_bytes()[i];
                assert_eq!(1, row.len());
                assert_eq!(to_compare, row[0]);
            }
        }
        println!("Buffer of 2");
        {
            let file = base_file_ref.switch_metadata(1, 0);
            let mut reader = BufferedReader::wrap(&file, 2);

            for i in 0..expected_str.len() {
                let row = reader.read_at(i).unwrap();
                let part = from_utf8(row).unwrap();

                println!("Read at [{}]: {}", i, part);
                let to_compare = expected_str.as_bytes()[i];
                assert_eq!(1, row.len());
                assert_eq!(to_compare, row[0]);
            }
        }
        println!("Hello offset");
        {
            let hello_size: usize = 5;
            let left_size = expected_str.len() - hello_size;
            let file = base_file_ref.switch_metadata(1, hello_size as u64);
            let mut reader = BufferedReader::wrap(&file, left_size);

            for i in 0..left_size {
                let row = reader.read_at(i).unwrap();

                let part = from_utf8(row).unwrap();
                println!("Read at [{}]: {}", i, part);

                let to_compare = expected_str.as_bytes()[i + hello_size];
                assert_eq!(1, row.len());
                assert_eq!(to_compare, row[0]);
            }
        }
    }

    #[test]
    fn test_write_buffer() -> anyhow::Result<()> {
        let expected_str = "Hello world";
        println!("Row count == 1");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, expected_str.len(), 0);
                let mut writer = BufferedWriter::wrap(&file, 1);

                writer.write(&expected_str.as_bytes()).unwrap();
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }
        println!("Row count > 1");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, expected_str.len(), 0);
                let mut writer = BufferedWriter::wrap(&file, 2);

                writer.write(&expected_str.as_bytes()).unwrap();
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }
        println!("sizeof a row == 1, row count == 1, for loop");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 1, 0);
                let mut writer = BufferedWriter::wrap(&file, 1);

                for i in expected_str.as_bytes() {
                    let temp: [u8; 1] = [*i];
                    writer.write(&temp)?;
                }
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }
        println!("sizeof a row == 1, row count == 1, write many");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 1, 0);
                let mut writer = BufferedWriter::wrap(&file, 1);

                writer.write_many(&expected_str.as_bytes()).unwrap();
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }
        println!("sizeof a row == 1, row count == size-2, for loop");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 1, 0);
                let mut writer = BufferedWriter::wrap(&file, expected_str.len() - 2);

                for i in expected_str.as_bytes() {
                    let temp: [u8; 1] = [*i];
                    writer.write(&temp)?;
                }
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }
        println!("sizeof a row == 1, row count == size-2, write many");
        {
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 1, 0);
                let mut writer = BufferedWriter::wrap(&file, expected_str.len() - 2);

                writer.write_many(&expected_str.as_bytes()).unwrap();
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, &receive);
        }

        Ok(())
    }

    #[test]
    fn test_write_buffer_rows() -> anyhow::Result<()> {
        {
            let expected_str = "Hello worl";
            println!("sizeof a row == 5, row count == 1");
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 5, 0);
                let mut writer = BufferedWriter::wrap(&file, 1);

                writer.write(&expected_str.as_bytes()[..5])?;
                writer.write(&expected_str.as_bytes()[5..])?;
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);

            assert_eq!(expected_str, receive);
        }
        {
            let expected_str = "Hello world";
            println!("sizeof a row == 5, row count == 3");
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 5, 0);
                let mut writer = BufferedWriter::wrap(&file, 3);

                writer.write(&expected_str.as_bytes()[..5])?;
                writer.write(&expected_str.as_bytes()[5..10])?;
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);
            let real_receive = from_utf8(&expected_str.as_bytes()[..10])?;

            assert_eq!(real_receive, receive);
        }
        Ok(())
    }

    #[test]
    fn test_write_buffer_with_offset() -> anyhow::Result<()> {
        {
            let expected_str = "Hello worl";
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            {
                let write_file = File::options()
                    .write(true)
                    .create(true)
                    .open(file_path.clone())
                    .unwrap();
                let file = FileReference::new_protected(write_file, 5, 5);
                let mut writer = BufferedWriter::wrap(&file, 1);

                writer.write(&expected_str.as_bytes()[..5])?;
                writer.write(&expected_str.as_bytes()[5..])?;
            }
            let mut reader = File::options().read(true).open(file_path.clone()).unwrap();

            let mut receive = String::new();
            reader.read_to_string(&mut receive)?;

            println!("Has write: {}", receive);
            assert_eq!(5 + expected_str.len(), receive.len());

            assert_eq!(expected_str, &receive[5..]);

            {
                let local_file = File::options().read(true).open(file_path.clone()).unwrap();
                let file = FileReference::new_protected(local_file, 5, 5);

                assert_eq!(5 + expected_str.len(), file.file_size() as usize);
                assert_eq!(2, file.row_count());
            }

            let file_length = fs::metadata(file_path.clone())?.len();

            assert_eq!(5 + expected_str.len(), file_length as usize);
        }
        Ok(())
    }

    #[test]
    fn test_read_write() -> anyhow::Result<()> {
        {
            let expected_str = "Hello worl";
            let dir = tempfile::tempdir().unwrap();
            let file_path = dir.path().join("temp_file_buffer_writer.temp");
            let file = FileReference::from_path(file_path, 5, 0, false)?;
            {
                let mut writer = file.writer(1);

                writer.write(&expected_str.as_bytes()[..5])?;
                writer.write(&expected_str.as_bytes()[5..])?;
            }
            {
                let mut reader = file.reader(1);

                let expected_as_byte = expected_str.as_bytes();
                let at_0 = reader.read_at(0)?;

                println!("At_0 : {}", from_utf8(at_0)?);

                assert_eq!(&expected_as_byte[..file.size_of_a_row], at_0);

                let at_1 = reader.read_at(1)?;

                println!("At_1 : {}", from_utf8(at_1)?);

                assert_eq!(
                    &expected_as_byte[file.size_of_a_row..2 * file.size_of_a_row],
                    at_1
                );
            }

            assert_eq!(10, file.file_size());
            assert_eq!(2, file.row_count());
            {
                let with_offset = file.switch_metadata(2, 5);
                let mut reader = with_offset.reader(1);

                let expected_as_byes = expected_str.as_bytes();

                let at_0 = reader.read_at(0)?;
                println!("At_0 : {}", from_utf8(at_0)?);

                assert_eq!(&expected_as_byes[5..7], at_0);

                let at_1 = reader.read_at(1)?;
                println!("At_1 : {}", from_utf8(at_1)?);

                assert_eq!(&expected_as_byes[7..9], at_1);

                assert_eq!(10, with_offset.file_size());
                assert_eq!(2, with_offset.row_count());
            }
            {
                let next = file.switch_metadata(1, 0);
                assert_eq!(10, next.file_size());
                assert_eq!(10, next.row_count());
            }
        }
        Ok(())
    }
}
