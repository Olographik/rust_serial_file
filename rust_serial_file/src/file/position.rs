use crate::{
    file::{FileNeededTrait, struct_file::StructBufferedReader},
    serialized_struct::SerializedStruct,
    utils::binary_search::{SearchIndex, SearchQuery, comparator_dicotomic_search},
};

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
pub enum Position {
    None,
    Exact(usize),
}

impl Into<usize> for Position {
    fn into(self) -> usize {
        match self {
            Position::None => 0,
            Position::Exact(i) => i,
        }
    }
}
impl Into<usize> for &Position {
    fn into(self) -> usize {
        match self {
            Position::None => 0,
            Position::Exact(i) => *i,
        }
    }
}
impl From<usize> for Position {
    fn from(value: usize) -> Self {
        Position::Exact(value)
    }
}

impl Into<Position> for SearchIndex {
    fn into(self) -> Position {
        match self {
            SearchIndex::None => Position::None,
            SearchIndex::Before(i) => Position::Exact(i),
            SearchIndex::Exact(i) => Position::Exact(i),
            SearchIndex::After(i) => Position::Exact(i),
        }
    }
}

pub fn binary_look_up_file_struct<S, F, K>(
    file: &mut StructBufferedReader<S, F>,
    target: &K,
    query: SearchQuery,
) -> SearchIndex
where
    S: SerializedStruct + PartialOrd<K>,
    F: FileNeededTrait,
{
    let row_count = file.row_count();
    comparator_dicotomic_search(
        |i| {
            let query_value = file.read_value_at(i).unwrap();
            query_value.partial_cmp(target).unwrap()
        },
        row_count,
        query,
    )
}

#[cfg(test)]
mod test {
    use crate::{file::FileReference, serialized_struct};
    use serialized_struct_derive::SerializedStruct;

    use crate::file::{position::binary_look_up_file_struct, struct_file::StructFile};
    use crate::utils::binary_search::{SearchIndex, SearchQuery};

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
            let x = 4;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::Exact);

            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Exact(3), pos);

            let x = 4;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::After(4), pos);

            let x = 4;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Before(2), pos);

            let x = 4;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Exact(3), pos);

            let x = 4;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Exact(3), pos);

            let x = 7;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Before(5), pos);

            let x = 7;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::Before(5), pos);

            let x = 7;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::Exact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 7;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 7;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 0;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 0;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 0;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::Exact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::None, pos);

            let x = 0;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOnly);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::After(0), pos);

            let x = 0;
            let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOrExact);
            println!("Found position {:?}", pos);
            assert_eq!(SearchIndex::After(0), pos);
        }

        Ok(())
    }

    // #[test]
    // fn test_look_up_struct_file_2() -> anyhow::Result<()> {
    //     let dir = tempfile::tempdir().unwrap();

    //     let file_path = dir.path().join("temp_file_buffer_reader.temp");
    //     let expected_structs = [
    //         TestStruct {
    //             time: 2,
    //             value: 64.23,
    //         },
    //         TestStruct {
    //             time: 4,
    //             value: 543.2,
    //         },
    //         TestStruct {
    //             time: 6,
    //             value: 321421.2,
    //         },
    //         TestStruct {
    //             time: 8,
    //             value: 12431.2,
    //         },
    //     ];

    //     let file_ref = FileReference::free_from_path(file_path, 0, 0, false)?;
    //     let file_ref = StructFile::wrap(file_ref);

    //     {
    //         let mut write_file = file_ref.writer(expected_structs.len());
    //         //create a temp file
    //         for i in expected_structs {
    //             write_file.write_value(&i)?;
    //         }
    //     }
    //     {
    //         assert_eq!(expected_structs.len(), file_ref.row_count());
    //     }

    //     {
    //         let mut reader = file_ref.reader(1);

    //         let x = 3;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::Exact);
    //         println!("Found position {:?}", pos);
    //         assert_eq!(Position::None, pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(0, pos_int);

    //         let x = 5;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOnly);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(1, pos_int);

    //         let x = 5;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOrExact);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(1, pos_int);

    //         let x = 5;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOnly);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(2, pos_int);

    //         let x = 5;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOrExact);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(2, pos_int);

    //         let x = 4;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::Exact);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(1, pos_int);

    //         let x = 4;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOnly);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(0, pos_int);

    //         let x = 4;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::BeforeOrExact);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(1, pos_int);

    //         let x = 4;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOnly);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(2, pos_int);

    //         let x = 4;
    //         let pos = binary_look_up_file_struct(&mut reader, &x, SearchQuery::AfterOrExact);
    //         println!("Found position {:?}", pos);
    //         let pos_int: usize = pos.into();
    //         assert_eq!(1, pos_int);
    //     }

    //     Ok(())
    // }
}
