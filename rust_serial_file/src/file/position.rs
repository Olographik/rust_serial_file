use std::{fmt::Debug, ops::Index};

use crate::{
    file::{struct_file::StructBufferedReader, FileNeededTrait},
    serialized_struct::SerializedStruct,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    Exact(usize),
    Before(usize),
    After(usize),
    None,
    BeforeStart,
}

impl Position {
    pub fn valid(&self) -> bool {
        match self {
            Position::None => false,
            _ => true,
        }
    }

    pub fn before(&self) -> Position {
        if !self.valid() {
            return Position::None;
        }
        let idx = self.into();
        if idx == 0 {
            return Position::BeforeStart;
        }
        return Position::Before(idx);
    }

    pub fn after(&self) -> Position {
        if !self.valid() {
            return Position::None;
        }
        let idx = self.into();
        return Position::After(idx);
    }
}

impl Into<usize> for Position {
    fn into(self) -> usize {
        match self {
            Position::Exact(v) => v,
            Position::Before(v) => v - 1,
            Position::After(v) => v + 1,
            Position::None => 0,
            Position::BeforeStart => 0,
        }
    }
}
impl From<&Position> for usize {
    fn from(value: &Position) -> Self {
        let copy = value.clone();
        copy.into()
    }
}

impl From<usize> for Position {
    fn from(value: usize) -> Self {
        Position::Exact(value)
    }
}

pub trait PosMatcher<B>
where
    B: Index<usize>,
    B::Output: Sized,
{
    fn try_get(&self, pos: &Position, max_len: usize) -> Option<&B::Output>;
}

impl<B> PosMatcher<B> for B
where
    B: Index<usize>,
    B::Output: Sized,
{
    fn try_get(&self, pos: &Position, max_len: usize) -> Option<&B::Output> {
        if *pos == Position::None {
            return None;
        }
        let idx: usize = pos.into();
        if idx >= max_len as usize {
            None
        } else {
            Some(&self[idx])
        }
    }
}

#[derive(Debug)]
pub enum Query<Q> {
    Exact(Q),
    BeforeOnly(Q),
    AfterOnly(Q),
    ExactOrBefore(Q),
    ExactOrAfter(Q),
}

impl<Q> Query<Q> {
    pub fn query_value(&self) -> &Q {
        match self {
            Query::Exact(q) => q,
            Query::BeforeOnly(q) => q,
            Query::AfterOnly(q) => q,
            Query::ExactOrBefore(q) => q,
            Query::ExactOrAfter(q) => q,
        }
    }

    pub fn resolve_position(&self, pos: Position) -> Position {
        match pos {
            Position::Exact(_) => match self {
                Query::Exact(_) => pos,
                Query::BeforeOnly(_) => pos.before(),
                Query::AfterOnly(_) => pos.after(),
                Query::ExactOrBefore(_) => pos,
                Query::ExactOrAfter(_) => pos,
            },
            Position::Before(_) => match self {
                Query::Exact(_) => Position::None,
                Query::BeforeOnly(_) => pos,
                Query::AfterOnly(_) => pos.after(), // assume the exact pos was not found
                Query::ExactOrBefore(_) => pos,
                Query::ExactOrAfter(_) => pos.after(), // assume the exact pos was not found
            },
            Position::After(_) => match self {
                Query::Exact(_) => Position::None,
                Query::BeforeOnly(_) => pos.before(), // assume the exact pos was not found
                Query::AfterOnly(_) => pos,
                Query::ExactOrBefore(_) => pos.before(), // assume the exact pos was not found
                Query::ExactOrAfter(_) => pos,
            },
            Position::None => pos,
            Position::BeforeStart => match self {
                Query::Exact(_) => Position::None,
                Query::BeforeOnly(_) => pos,
                Query::AfterOnly(_) => Position::None,
                Query::ExactOrBefore(_) => pos,
                Query::ExactOrAfter(_) => Position::None,
            },
        }
    }
}

pub fn binary_look_up<B, Q>(buf: &B, buf_len: usize, query: &Query<Q>) -> Position
where
    B: Index<usize>,
    B::Output: PartialEq<Q> + PartialOrd<Q>,
{
    let mut low: usize = 0;

    let mut high = buf_len - 1;
    if low == high {
        return Position::None;
    }

    let query_value = query.query_value();

    let low_val = &buf[low];
    if low_val == query_value {
        return query.resolve_position(Position::Exact(low));
    } else if low_val > query_value {
        return match query {
            Query::Exact(_) => Position::None,
            Query::BeforeOnly(_) => Position::None,
            Query::AfterOnly(_) => Position::BeforeStart,
            Query::ExactOrBefore(_) => Position::None,
            Query::ExactOrAfter(_) => Position::BeforeStart,
        };
    }

    let high_val = &buf[high];
    if high_val == query_value {
        return query.resolve_position(Position::Exact(high));
    } else if high_val < query_value {
        return match query {
            Query::Exact(_) => Position::None,
            Query::BeforeOnly(_) => Position::Before(high + 1),
            Query::AfterOnly(_) => Position::None,
            Query::ExactOrBefore(_) => Position::Before(high + 1),
            Query::ExactOrAfter(_) => Position::None,
        };
    }

    let mut current_position = Position::None;
    while high > low {
        let half = (high + low) / 2;
        let v = &buf[half];

        if v == query_value {
            return query.resolve_position(Position::Exact(half));
        } else if v > query_value {
            current_position = Position::Before(half);
            high = half;
        } else {
            current_position = Position::After(half);
            low = half + 1;
        }
    }

    query.resolve_position(current_position)
}

pub fn struct_reader_look_up<S, F, Q>(
    buf: &mut StructBufferedReader<S, F>,
    query: &Query<Q>,
) -> Position
where
    S: SerializedStruct + PartialEq<Q> + PartialOrd<Q>,
    F: FileNeededTrait,
{
    let mut low: usize = 0;

    let buf_len = buf.file.row_count() as usize;
    let mut high = buf_len - 1;
    if low == high {
        return Position::None;
    }

    let query_value = query.query_value();

    let low_val = if let Ok(v) = buf.read_value_at(low) {
        v
    } else {
        return Position::None;
    };
    if &low_val == query_value {
        return query.resolve_position(Position::Exact(low));
    } else if &low_val > query_value {
        return match query {
            Query::Exact(_) => Position::None,
            Query::BeforeOnly(_) => Position::None,
            Query::AfterOnly(_) => Position::BeforeStart,
            Query::ExactOrBefore(_) => Position::None,
            Query::ExactOrAfter(_) => Position::BeforeStart,
        };
    }

    let high_val = if let Ok(v) = buf.read_value_at(high) {
        v
    } else {
        return Position::None;
    };
    if &high_val == query_value {
        return query.resolve_position(Position::Exact(high));
    } else if &high_val < query_value {
        return match query {
            Query::Exact(_) => Position::None,
            Query::BeforeOnly(_) => Position::Before(high + 1),
            Query::AfterOnly(_) => Position::None,
            Query::ExactOrBefore(_) => Position::Before(high + 1),
            Query::ExactOrAfter(_) => Position::None,
        };
    }

    let mut current_position = Position::None;
    while high > low {
        let half = (high + low) / 2;
        let v = if let Ok(v) = buf.read_value_at(half) {
            v
        } else {
            return Position::None;
        };

        if &v == query_value {
            return query.resolve_position(Position::Exact(half));
        } else if &v > query_value {
            current_position = Position::Before(half);
            high = half;
        } else {
            current_position = Position::After(half);
            low = half + 1;
        }
    }

    query.resolve_position(current_position)
}

#[cfg(test)]
mod test {
    use crate::{
        file::{position::struct_reader_look_up, FileReference},
        serialized_struct,
    };
    use serialized_struct_derive::SerializedStruct;

    use crate::file::{
        position::{binary_look_up, PosMatcher, Position, Query},
        struct_file::StructFile,
    };

    #[test]
    fn test_look_up() {
        let data = [1, 2, 3, 4, 5, 6];
        let query = Query::Exact(4);
        let pos = binary_look_up(&data, data.len(), &query);

        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(3, pos_int);

        let query = Query::AfterOnly(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(4, pos_int);

        let query = Query::BeforeOnly(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(2, pos_int);

        let query = Query::ExactOrAfter(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(3, pos_int);

        let query = Query::ExactOrBefore(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(3, pos_int);

        let query = Query::ExactOrBefore(7);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(5, pos_int);

        let query = Query::BeforeOnly(7);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(5, pos_int);

        let query = Query::Exact(7);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::AfterOnly(7);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::ExactOrAfter(7);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::ExactOrBefore(0);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::BeforeOnly(0);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::Exact(0);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::AfterOnly(0);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::ExactOrAfter(0);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = (&pos).into();
        assert_eq!(0, pos_int);

        let query = Query::Exact(0);
        let pos = binary_look_up(&data, data.len(), &query);
        let v = data.try_get(&pos, data.len());
        println!("{:?}", v);
        assert_eq!(None, v);
        let query = Query::ExactOrAfter(0);
        let pos = binary_look_up(&data, data.len(), &query);
        let v = data.try_get(&pos, data.len());
        println!("{:?}", v);
        assert_ne!(None, v);
        let v = if let Some(v) = v {
            *v
        } else {
            panic!("Value must be some");
        };
        assert_eq!(1, v);

        let query = Query::ExactOrBefore(0);
        let pos = binary_look_up(&data, data.len(), &query);
        let v = data.try_get(&pos, data.len());
        println!("{:?}", v);
        assert_eq!(None, v);
    }

    #[test]
    fn test_look_up_2() {
        let data = [2, 4, 6, 8];

        let query = Query::Exact(3);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        assert_eq!(Position::None, pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::BeforeOnly(5);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(1, pos_int);

        let query = Query::ExactOrBefore(5);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(1, pos_int);

        let query = Query::AfterOnly(5);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(2, pos_int);

        let query = Query::ExactOrAfter(5);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(2, pos_int);

        let query = Query::Exact(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(1, pos_int);

        let query = Query::BeforeOnly(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(0, pos_int);

        let query = Query::ExactOrBefore(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(1, pos_int);

        let query = Query::AfterOnly(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(2, pos_int);

        let query = Query::ExactOrAfter(4);
        let pos = binary_look_up(&data, data.len(), &query);
        println!("Found position {:?}", pos);
        let pos_int: usize = pos.into();
        assert_eq!(1, pos_int);
    }

    #[derive(Debug, Clone, Copy, PartialEq, SerializedStruct)]
    struct TestStruct {
        time: u64,
        value: f32,
    }

    impl PartialEq<u64> for TestStruct {
        fn eq(&self, other: &u64) -> bool {
            self.time == *other
        }
        fn ne(&self, other: &u64) -> bool {
            !self.eq(other)
        }
    }
    impl PartialOrd<u64> for TestStruct {
        fn partial_cmp(&self, other: &u64) -> Option<std::cmp::Ordering> {
            if &self.time > other {
                Some(std::cmp::Ordering::Greater)
            } else if &self.time < other {
                Some(std::cmp::Ordering::Less)
            } else {
                Some(std::cmp::Ordering::Equal)
            }
        }
    }

    #[test]
    fn test_look_up_struct_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_structs = [
            TestStruct {
                time: 1,
                value: 64.23,
            },
            TestStruct {
                time: 2,
                value: 543.2,
            },
            TestStruct {
                time: 3,
                value: 321421.2,
            },
            TestStruct {
                time: 4,
                value: 12431.2,
            },
            TestStruct {
                time: 5,
                value: 123412451.2,
            },
            TestStruct {
                time: 6,
                value: 84312.12,
            },
        ];

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file = file_ref.writer(expected_structs.len());
            //create a temp file
            for i in expected_structs {
                write_file.write_value(&i)?;
            }
        }
        {
            assert_eq!(expected_structs.len(), file_ref.row_count());
        }

        {
            let mut reader = file_ref.reader(1);
            let query = Query::Exact(4);
            let pos = struct_reader_look_up(&mut reader, &query);

            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(3, pos_int);

            let query = Query::AfterOnly(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(4, pos_int);

            let query = Query::BeforeOnly(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(2, pos_int);

            let query = Query::ExactOrAfter(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(3, pos_int);

            let query = Query::ExactOrBefore(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(3, pos_int);

            let query = Query::ExactOrBefore(7);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(5, pos_int);

            let query = Query::BeforeOnly(7);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(5, pos_int);

            let query = Query::Exact(7);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::AfterOnly(7);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::ExactOrAfter(7);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::ExactOrBefore(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::BeforeOnly(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::Exact(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::AfterOnly(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::ExactOrAfter(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = (&pos).into();
            assert_eq!(0, pos_int);

            let query = Query::Exact(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            let v = reader.try_get(&pos);
            println!("{:?}", v);
            assert_eq!(None, v);
            let query = Query::ExactOrAfter(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            let v = reader.try_get(&pos);
            println!("{:?}", v);
            assert_ne!(None, v);
            let v = if let Some(v) = v {
                v
            } else {
                panic!("Value must be some");
            };
            assert_eq!(expected_structs[0], v);

            let query = Query::ExactOrBefore(0);
            let pos = struct_reader_look_up(&mut reader, &query);
            let v = reader.try_get(&pos);
            println!("{:?}", v);
            assert_eq!(None, v);
        }

        Ok(())
    }

    #[test]
    fn test_look_up_struct_file_2() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().unwrap();

        let file_path = dir.path().join("temp_file_buffer_reader.temp");
        let expected_structs = [
            TestStruct {
                time: 2,
                value: 64.23,
            },
            TestStruct {
                time: 4,
                value: 543.2,
            },
            TestStruct {
                time: 6,
                value: 321421.2,
            },
            TestStruct {
                time: 8,
                value: 12431.2,
            },
        ];

        let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
        let file_ref = StructFile::wrap(file_ref);

        {
            let mut write_file = file_ref.writer(expected_structs.len());
            //create a temp file
            for i in expected_structs {
                write_file.write_value(&i)?;
            }
        }
        {
            assert_eq!(expected_structs.len(), file_ref.row_count());
        }

        {
            let mut reader = file_ref.reader(1);

            let query = Query::Exact(3);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            assert_eq!(Position::None, pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::BeforeOnly(5);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(1, pos_int);

            let query = Query::ExactOrBefore(5);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(1, pos_int);

            let query = Query::AfterOnly(5);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(2, pos_int);

            let query = Query::ExactOrAfter(5);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(2, pos_int);

            let query = Query::Exact(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(1, pos_int);

            let query = Query::BeforeOnly(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(0, pos_int);

            let query = Query::ExactOrBefore(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(1, pos_int);

            let query = Query::AfterOnly(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(2, pos_int);

            let query = Query::ExactOrAfter(4);
            let pos = struct_reader_look_up(&mut reader, &query);
            println!("Found position {:?}", pos);
            let pos_int: usize = pos.into();
            assert_eq!(1, pos_int);
        }

        Ok(())
    }
}
