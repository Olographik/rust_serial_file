use anyhow::{Ok, anyhow};

use crate::file::position::Position;

pub trait DataContainer<T> {
    fn add_into(&mut self, value: T) -> anyhow::Result<()>;

    fn write_at(&mut self, positon: usize, value: T) -> anyhow::Result<()>;

    fn insert_at(&mut self, positon: usize, value: T) -> anyhow::Result<()>;

    fn read_at(&mut self, positon: usize) -> Option<T>
    where
        T: Clone;

    fn find<K>(&mut self, key: &K) -> Option<T>
    where
        T: PartialEq<K> + PartialOrd<K>;

    fn find_index_for_object(&mut self, object: &T) -> Position;
    fn find_index<K>(&mut self, key: &K) -> Position;

    fn auto_insert(&mut self, value: T) -> anyhow::Result<()>
    where
        T: PartialEq<T> + PartialOrd<T>;

    type Iter<'a>: Iterator<Item = &'a T>
    where
        Self: 'a,
        T: 'a;

    fn iter_items(&self) -> Self::Iter<'_>;

    fn item_count(&self) -> usize;

    fn has_item(&self) -> bool {
        self.item_count() != 0
    }
}

impl<T> DataContainer<T> for Vec<T> {
    type Iter<'a>
        = std::slice::Iter<'a, T>
    where
        Self: 'a,
        T: 'a;

    fn iter_items(&self) -> Self::Iter<'_> {
        self.iter()
    }

    fn item_count(&self) -> usize {
        self.len()
    }

    fn add_into(&mut self, value: T) -> anyhow::Result<()> {
        self.push(value);
        Ok(())
    }

    fn write_at(&mut self, positon: usize, value: T) -> anyhow::Result<()> {
        if positon > self.len() {
            return Err(anyhow!("{} Out of bound", positon));
        }
        self[positon] = value;
        Ok(())
    }

    fn insert_at(&mut self, positon: usize, value: T) -> anyhow::Result<()> {
        if positon > self.len() {
            return Err(anyhow!("{} Out of bound", positon));
        }
        self.insert(positon, value);
        Ok(())
    }

    fn read_at(&mut self, positon: usize) -> Option<T>
    where
        T: Clone,
    {
        self.get(positon).map(|e| e.clone())
    }

    fn find<K>(&mut self, key: &K) -> Option<T>
    where
        T: PartialEq<K> + PartialOrd<K>,
    {
        todo!()
    }

    fn find_index<K>(&mut self, key: &K) -> Position {
        todo!()
    }

    fn auto_insert(&mut self, value: T) -> anyhow::Result<()>
    where
        T: PartialEq<T> + PartialOrd<T>,
    {
        let position_to_insert = self.find_index_for_object(&value);

        todo!()
    }

    fn find_index_for_object(&mut self, object: &T) -> Position {
        todo!()
    }
}
