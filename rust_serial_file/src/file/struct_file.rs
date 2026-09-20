use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::{
    file::position::{Position, binary_look_up_file_struct},
    serialized_struct::SerializedStruct,
    utils::binary_search::{SearchIndex, SearchQuery},
};

use super::{BufferedReader, BufferedWriter, FileNeededTrait, FileReference};

#[derive(Debug, Clone)]
pub struct StructFile<S: SerializedStruct, F: FileNeededTrait> {
    pub file: FileReference<F>,
    _phantom: PhantomData<S>,
}

impl<S: SerializedStruct, F: FileNeededTrait> StructFile<S, F> {
    pub fn wrap(mut file: FileReference<F>) -> Self {
        file.switch_metadata_inplace(S::SIZE, file.offset);
        Self {
            file,
            _phantom: PhantomData,
        }
    }

    pub fn writer(&self, row_count: usize) -> StructBufferedWriter<S, F> {
        StructBufferedWriter::wrap(self, row_count)
    }
    pub fn reader(&self, row_count: usize) -> StructBufferedReader<S, F> {
        StructBufferedReader::wrap(self, row_count)
    }

    pub fn find_by_key<K>(&mut self, key: &K) -> SearchIndex
    where
        S: PartialOrd<K>,
    {
        let mut reader = self.reader(100);
        reader.find_by_key(key)
    }

    pub fn find_by_element(&mut self, element: &S) -> SearchIndex
    where
        S: PartialOrd<S>,
    {
        let mut reader = self.reader(100);
        reader.find_by_element(element)
    }

    pub fn append(&mut self, element: &S) -> anyhow::Result<()> {
        let mut writer = self.writer(1);
        writer.seek(self.row_count())?;
        writer.write_value(element)
    }

    pub fn write_value_at(&mut self, element: &S, index: usize) -> anyhow::Result<()> {
        let mut writer = self.writer(1);
        writer.write_value_at(index, element)
    }

    fn insert_element_at(&mut self, element: &S, index: usize) -> anyhow::Result<()> {
        let buffer_size = 100;
        let mut over_index = self.row_count();

        let mut end_index = self.row_count();

        over_index = if over_index > buffer_size {
            over_index - buffer_size
        } else {
            0
        };

        if over_index < index {
            over_index = index;
        }

        while over_index >= index {
            let mut temp_array: Vec<u8> = Vec::with_capacity(buffer_size * self.size_of_a_row);
            let mut reader = self.reader(buffer_size);
            for i in over_index..end_index {
                temp_array.extend(reader.read_at(i)?);
            }
            let mut writer = self.writer(buffer_size);
            writer.write_at(over_index + 1, temp_array.as_slice())?;

            if over_index == index {
                break;
            }
            end_index = over_index;
            over_index -= buffer_size;
            if over_index < index {
                over_index = index;
            }
        }

        self.write_value_at(element, index)
    }

    pub fn insert(&mut self, element: &S) -> anyhow::Result<()>
    where
        S: PartialOrd<S>,
    {
        let existing_element = self.find_by_element(element);
        match existing_element {
            SearchIndex::None => self.append(element),
            SearchIndex::Before(i) => self.insert_element_at(element, i + 1),
            SearchIndex::Exact(i) => self.write_value_at(element, i),
            SearchIndex::After(i) => self.insert_element_at(element, i),
        }
    }
}

impl<S: SerializedStruct, F: FileNeededTrait> Deref for StructFile<S, F> {
    type Target = FileReference<F>;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

impl<S: SerializedStruct, F: FileNeededTrait> DerefMut for StructFile<S, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file
    }
}

pub struct StructBufferedReader<S: SerializedStruct, T: FileNeededTrait> {
    backed_by: BufferedReader<T>,
    _phantom_data: PhantomData<S>,
}

impl<S: SerializedStruct, T: FileNeededTrait> StructBufferedReader<S, T> {
    pub fn wrap(file: &StructFile<S, T>, row_count: usize) -> Self {
        let l: &FileReference<T> = *&file;
        Self {
            backed_by: l.reader(row_count),
            _phantom_data: PhantomData,
        }
    }

    pub fn read_value_at(&mut self, index: usize) -> anyhow::Result<S> {
        let result = self.backed_by.read_at(index)?;
        Ok(S::from_le_bytes_only(result))
    }

    pub fn try_get(&mut self, position: &Position) -> Option<S> {
        if *position == Position::None {
            return None;
        }
        let idx = position.into();
        if idx >= self.file.row_count() {
            None
        } else {
            self.read_value_at(idx).ok()
        }
    }

    pub fn find_by_key<K>(&mut self, key: &K) -> SearchIndex
    where
        S: PartialOrd<K> + PartialEq<K>,
    {
        binary_look_up_file_struct(self, key, SearchQuery::Any)
    }

    pub fn find_by_element(&mut self, element: &S) -> SearchIndex
    where
        S: PartialOrd<S>,
    {
        binary_look_up_file_struct(self, element, SearchQuery::Any)
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> Deref for StructBufferedReader<S, T> {
    type Target = BufferedReader<T>;
    fn deref(&self) -> &Self::Target {
        &self.backed_by
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> DerefMut for StructBufferedReader<S, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.backed_by
    }
}

pub struct StructFileIterator<S: SerializedStruct, F: FileNeededTrait> {
    reader: StructBufferedReader<S, F>,
    index: usize,
    len: usize,
}

impl<S: SerializedStruct, T: FileNeededTrait> StructFileIterator<S, T> {
    pub fn iterator(reader: StructBufferedReader<S, T>) -> Self {
        return reader.into();
    }

    pub fn limited_iterator(reader: StructBufferedReader<S, T>, start: usize, end: usize) -> Self {
        Self {
            reader,
            index: start,
            len: end,
        }
    }

    pub fn limited_iterator_from_start(reader: StructBufferedReader<S, T>, end: usize) -> Self {
        Self {
            reader,
            index: 0,
            len: end,
        }
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> From<StructBufferedReader<S, T>>
    for StructFileIterator<S, T>
{
    fn from(value: StructBufferedReader<S, T>) -> Self {
        let len = value.file.row_count();
        Self {
            reader: value,
            index: 0,
            len,
        }
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> From<StructFile<S, T>> for StructFileIterator<S, T> {
    fn from(value: StructFile<S, T>) -> Self {
        let len = value.row_count();
        Self {
            reader: value.reader(100),
            index: 0,
            len,
        }
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> Iterator for StructFileIterator<S, T> {
    type Item = S;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.len {
            let i = self.index;
            self.index += 1;
            match self.reader.read_value_at(i) {
                Ok(e) => Some(e),
                Err(_) => None,
            }
        } else {
            None
        }
    }
}

pub struct StructBufferedWriter<S: SerializedStruct, T: FileNeededTrait> {
    backed_by: BufferedWriter<T>,
    _phantom_data: PhantomData<S>,
}

impl<S: SerializedStruct, T: FileNeededTrait> StructBufferedWriter<S, T> {
    pub fn wrap(file: &StructFile<S, T>, row_count: usize) -> Self {
        let writer = (file as &FileReference<T>).writer(row_count);
        Self {
            backed_by: writer,
            _phantom_data: PhantomData,
        }
    }

    fn increase_buffer(&mut self, rows: usize) -> usize {
        let buffer_in_pos = self.buffer.len();
        let size_of_a_row = self.file.size_of_a_row;
        unsafe {
            self.buffer.set_len(buffer_in_pos + size_of_a_row * rows);
        }
        buffer_in_pos
    }

    pub fn write_value_at(&mut self, index: usize, data: &S) -> anyhow::Result<()> {
        self.seek(index)?;

        let buffer_in_pos = self.increase_buffer(1);

        let buf = &mut self.buffer[buffer_in_pos..];

        data.encode(buf);
        // self.backed_by.write_at(index, &buf[..])
        Ok(())
    }

    pub fn write_value(&mut self, data: &S) -> anyhow::Result<()> {
        self.check_flush()?;

        let buffer_in_pos = self.increase_buffer(1);

        let buf = &mut self.buffer[buffer_in_pos..];

        data.encode(buf);
        // self.backed_by.write_at(index, &buf[..])
        Ok(())
    }

    pub fn write_many_value(&mut self, mut rows: &[S]) -> anyhow::Result<()> {
        let mut row_to_write = rows.len();

        let max_row_count = self.buffer.capacity() / self.file.size_of_a_row;

        while row_to_write > 0 {
            self.check_flush()?;
            let max = if row_to_write > max_row_count {
                max_row_count
            } else {
                row_to_write
            };

            let mut buffer_in = self.increase_buffer(max);
            for row in &rows[..max] {
                buffer_in += row.encode(&mut self.buffer[buffer_in..]);
            }
            rows = &rows[max..];

            row_to_write = rows.len();
        }

        Ok(())
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> Deref for StructBufferedWriter<S, T> {
    type Target = BufferedWriter<T>;
    fn deref(&self) -> &Self::Target {
        &self.backed_by
    }
}

impl<S: SerializedStruct, T: FileNeededTrait> DerefMut for StructBufferedWriter<S, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.backed_by
    }
}

#[cfg(test)]
mod test {

    use crate::{file::FileReference, serialized_struct};
    use serialized_struct_derive::SerializedStruct;

    use super::StructFile;

    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, SerializedStruct)]
    struct TestStruct {
        time: u64,
        value: f32,
    }

    #[test]
    fn test_struct_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_str = TestStruct {
            time: 10,
            value: 64.23,
        };
        let expected_2 = TestStruct {
            time: 20,
            value: 543.2,
        };

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file = file_ref.writer(4);
            //create a temp file
            write_file.write_value(&expected_str)?;
            write_file.write_value(&expected_2)?;
        }

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;

            println!("{expected_str:?} -> {result:?}");
            assert_eq!(expected_str, result);

            let result = reader.read_value_at(1)?;

            println!("{expected_2:?} -> {result:?}");
            assert_eq!(expected_2, result);
        }

        Ok(())
    }
    #[test]
    fn test_struct_file_many() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_str = TestStruct {
            time: 10,
            value: 64.23,
        };
        let expected_2 = TestStruct {
            time: 20,
            value: 543.2,
        };

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file = file_ref.writer(1);
            //create a temp file
            write_file.write_many_value(&[expected_str, expected_2])?;
        }

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;

            println!("{expected_str:?} -> {result:?}");
            assert_eq!(expected_str, result);

            let result = reader.read_value_at(1)?;

            println!("{expected_2:?} -> {result:?}");
            assert_eq!(expected_2, result);
        }

        Ok(())
    }

    fn struct_create(time: u64) -> TestStruct {
        TestStruct {
            time: time,
            value: time as f32 / 10.0,
        }
    }

    #[test]
    fn test_struct_file_insert_middle() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");

        let mut values_to_write = Vec::new();
        for i in 0..400 {
            values_to_write.push(struct_create((i + 1) * 10));
        }

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let mut file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file: super::StructBufferedWriter<
                TestStruct,
                std::sync::Arc<std::fs::File>,
            > = file_ref.writer(100);
            //create a temp file
            values_to_write
                .iter()
                .for_each(|v| write_file.write_value(&v).unwrap());
        }

        let element = TestStruct {
            time: 2102,
            value: 50.0,
        };
        file_ref.insert(&element)?;

        assert_eq!(401, file_ref.row_count());

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;
            let expected = values_to_write[0];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(210)?;

            println!("{element:?} -> {result:?}");
            assert_eq!(element, result);

            let result = reader.read_value_at(1)?;
            let expected = values_to_write[1];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(209)?;
            let expected = values_to_write[209];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(211)?;
            let expected = values_to_write[210];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let mut offset = 0;
            for i in 0..reader.row_count() {
                let result = reader.read_value_at(i)?;
                if i == 210 {
                    assert_eq!(element, result);
                    offset = 1;
                } else {
                    assert_eq!(values_to_write[i - offset], result);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_struct_file_insert_beginning() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");

        let mut values_to_write = Vec::new();
        for i in 0..400 {
            values_to_write.push(struct_create((i + 1) * 10));
        }

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let mut file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file: super::StructBufferedWriter<
                TestStruct,
                std::sync::Arc<std::fs::File>,
            > = file_ref.writer(100);
            //create a temp file
            values_to_write
                .iter()
                .for_each(|v| write_file.write_value(&v).unwrap());
        }

        let element = TestStruct {
            time: 0,
            value: 50.0,
        };
        file_ref.insert(&element)?;

        assert_eq!(401, file_ref.row_count());

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;
            let expected = element;

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(1)?;

            let expected = values_to_write[0];
            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(2)?;
            let expected = values_to_write[1];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let mut offset = 0;
            for i in 0..reader.row_count() {
                let result = reader.read_value_at(i)?;
                if i == 0 {
                    assert_eq!(element, result);
                    offset = 1;
                } else {
                    assert_eq!(values_to_write[i - offset], result);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_struct_file_insert_end() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");

        let mut values_to_write = Vec::new();
        for i in 0..400 {
            values_to_write.push(struct_create((i + 1) * 10));
        }

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let mut file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file: super::StructBufferedWriter<
                TestStruct,
                std::sync::Arc<std::fs::File>,
            > = file_ref.writer(100);
            //create a temp file
            values_to_write
                .iter()
                .for_each(|v| write_file.write_value(&v).unwrap());
        }

        let element = TestStruct {
            time: 9999999999,
            value: 50.0,
        };
        file_ref.insert(&element)?;

        assert_eq!(401, file_ref.row_count());

        {
            let mut reader = file_ref.reader(1);

            let result = reader.read_value_at(0)?;
            let expected = values_to_write[0];

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(1)?;

            let expected = values_to_write[1];
            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            let result = reader.read_value_at(reader.row_count() - 1)?;
            let expected = element;

            println!("{expected:?} -> {result:?}");
            assert_eq!(expected, result);

            for i in 0..reader.row_count() {
                let result = reader.read_value_at(i)?;
                if i == reader.row_count() - 1 {
                    assert_eq!(element, result);
                } else {
                    assert_eq!(values_to_write[i], result);
                }
            }
        }

        Ok(())
    }
}
