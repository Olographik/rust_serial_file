use std::ops::Index;

use utils::search::{
    binary_search_v2::{dicotomic_search_fn, dicotomic_search_with_context_fn},
    search_type::{SearchContext, SearchIndexes},
};

pub trait SerialDataContainer<T>: Index<usize, Output = T> {
    type Iter<'a>: Iterator<Item = &'a T>
    where
        Self: 'a,
        T: 'a;

    type IterMut<'a>: Iterator<Item = &'a mut T>
    where
        Self: 'a,
        T: 'a;

    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;

    fn serial_push(&mut self, value: T);
    fn serial_insert(&mut self, index: usize, value: T);
    fn serial_remove(&mut self, index: usize) -> T;
    fn serial_clear(&mut self);
    fn serial_truncate(&mut self, len: usize);

    fn serial_get(&mut self, index: usize) -> Option<&T>;
    fn serial_first(&mut self) -> Option<&T>;
    fn serial_last(&mut self) -> Option<&T>;

    fn serial_iter(&mut self) -> Self::Iter<'_>;
    fn serial_contains(&mut self, value: &T) -> bool
    where
        T: PartialOrd,
    {
        dicotomic_search_fn(|i| &self[i], value, self.len())
            .exact
            .is_some()
    }

    fn write_at(&mut self, index: usize, value: T);

    fn find_index_for_object(&mut self, object: &T) -> SearchIndexes
    where
        T: PartialOrd<T>,
    {
        dicotomic_search_fn(|i| &self[i], object, self.len())
    }

    fn find_index<K>(&mut self, key: &K) -> SearchIndexes
    where
        T: PartialOrd<K>,
    {
        dicotomic_search_fn(|i| &self[i], key, self.len())
    }

    fn find<K>(&mut self, key: &K) -> Option<&T>
    where
        T: PartialOrd<K>,
    {
        let index = self.find_index(&key);
        if let Some(idx) = index.exact {
            Some(&self[idx])
        } else {
            None
        }
    }

    fn find_index_for_object_with_context(
        &mut self,
        object: &T,
        context: &mut SearchContext,
    ) -> SearchIndexes
    where
        T: PartialOrd<T>,
    {
        dicotomic_search_with_context_fn(|i| &self[i], object, self.len(), context)
    }

    fn find_index_with_context<K>(&mut self, key: &K, context: &mut SearchContext) -> SearchIndexes
    where
        T: PartialOrd<K>,
    {
        dicotomic_search_with_context_fn(|i| &self[i], key, self.len(), context)
    }

    fn find_with_context<K>(&mut self, key: &K, context: &mut SearchContext) -> Option<&T>
    where
        T: PartialOrd<K>,
    {
        let index = self.find_index_with_context(&key, context);
        if let Some(idx) = index.exact {
            Some(&self[idx])
        } else {
            None
        }
    }

    fn auto_insert(&mut self, value: T)
    where
        T: PartialOrd<T>,
    {
        let index = self.find_index_for_object(&value);
        if let Some(idx) = index.exact {
            self.write_at(idx, value);
        } else if let Some(idx) = index.after {
            self.serial_insert(idx, value);
        } else {
            // After is None:
            // case 1: before is None => buffer empty
            // case 2: before is Some => at the end of the buffer
            self.serial_push(value);
        }
    }
}

impl<T> SerialDataContainer<T> for Vec<T> {
    type Iter<'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    type IterMut<'a>
        = std::slice::IterMut<'a, T>
    where
        T: 'a;

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    fn serial_push(&mut self, value: T) {
        Vec::push(self, value);
    }

    fn serial_insert(&mut self, index: usize, value: T) {
        Vec::insert(self, index, value);
    }

    fn serial_remove(&mut self, index: usize) -> T {
        Vec::remove(self, index)
    }

    fn serial_clear(&mut self) {
        Vec::clear(self);
    }

    fn serial_truncate(&mut self, len: usize) {
        Vec::truncate(self, len);
    }

    fn serial_get(&mut self, index: usize) -> Option<&T> {
        self.get(index)
    }

    fn serial_first(&mut self) -> Option<&T> {
        self.first()
    }

    fn serial_last(&mut self) -> Option<&T> {
        self.last()
    }

    fn serial_iter(&mut self) -> Self::Iter<'_> {
        self.iter()
    }

    fn write_at(&mut self, index: usize, value: T) {
        self[index] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::SerialDataContainer;
    use utils::search::search_type::{SearchContext, SearchIndexes};

    fn get_and_set<C: SerialDataContainer<i32>>(container: &mut C) -> i32 {
        let first = container[0];
        container.write_at(0, 11);
        first
    }

    #[test]
    fn data_container_exposes_indexing() {
        let mut values = vec![10];

        assert_eq!(get_and_set(&mut values), 10);
        assert_eq!(values[0], 11);
    }

    #[test]
    fn serial_data_container_exposes_vec_operations() {
        let mut values = vec![10, 20];
        assert_eq!(SerialDataContainer::len(&values), 2);
        assert!(!SerialDataContainer::is_empty(&values));
        assert_eq!(SerialDataContainer::serial_get(&mut values, 1), Some(&20));
        assert_eq!(SerialDataContainer::serial_get(&mut values, 2), None);
        assert_eq!(SerialDataContainer::serial_first(&mut values), Some(&10));
        assert_eq!(SerialDataContainer::serial_last(&mut values), Some(&20));
        assert!(SerialDataContainer::serial_contains(&mut values, &10));
        assert!(!SerialDataContainer::serial_contains(&mut values, &15));
        assert_eq!(
            SerialDataContainer::serial_iter(&mut values)
                .copied()
                .collect::<Vec<_>>(),
            [10, 20]
        );

        SerialDataContainer::serial_push(&mut values, 30);
        SerialDataContainer::serial_insert(&mut values, 1, 15);
        assert_eq!(SerialDataContainer::serial_remove(&mut values, 1), 15);
        SerialDataContainer::serial_truncate(&mut values, 2);
        values.write_at(0, 5);
        assert_eq!(values, [5, 20]);

        SerialDataContainer::serial_clear(&mut values);
        assert!(SerialDataContainer::is_empty(&values));
        assert_eq!(SerialDataContainer::serial_first(&mut values), None);
        assert_eq!(SerialDataContainer::serial_last(&mut values), None);
    }

    #[test]
    fn serial_data_container_searches_with_and_without_context() {
        let mut values = vec![10, 20, 30, 40, 50];

        assert_eq!(
            SerialDataContainer::find_index_for_object(&mut values, &30),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3),
            }
        );
        assert_eq!(
            SerialDataContainer::find_index(&mut values, &30),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3),
            }
        );
        assert_eq!(SerialDataContainer::find(&mut values, &30), Some(&30));
        assert_eq!(
            SerialDataContainer::find_index(&mut values, &25),
            SearchIndexes {
                before: Some(1),
                exact: None,
                after: Some(2),
            }
        );
        assert_eq!(
            SerialDataContainer::find_index(&mut values, &5),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0),
            }
        );
        assert_eq!(
            SerialDataContainer::find_index(&mut values, &60),
            SearchIndexes {
                before: Some(4),
                exact: None,
                after: None,
            }
        );

        let mut context = SearchContext::none();
        assert_eq!(
            SerialDataContainer::find_index_with_context(&mut values, &30, &mut context),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3),
            }
        );
        assert_eq!(context, SearchContext::of(2));
        assert_eq!(
            SerialDataContainer::find_index_for_object_with_context(&mut values, &20, &mut context),
            SearchIndexes {
                before: Some(0),
                exact: Some(1),
                after: Some(2),
            }
        );
        assert_eq!(context, SearchContext::of(1));
        assert_eq!(
            SerialDataContainer::find_index_with_context(&mut values, &20, &mut context),
            SearchIndexes {
                before: Some(0),
                exact: Some(1),
                after: Some(2),
            }
        );
        assert_eq!(context, SearchContext::of(1));

        let mut context = SearchContext::of(4);
        assert_eq!(
            SerialDataContainer::find_with_context(&mut values, &40, &mut context),
            Some(&40)
        );
        assert_eq!(context, SearchContext::of(3));
    }

    #[test]
    fn serial_data_container_searches_empty_and_single_item_containers() {
        let mut empty: Vec<i32> = Vec::new();
        assert_eq!(
            SerialDataContainer::find_index(&mut empty, &10),
            SearchIndexes::none()
        );
        assert_eq!(SerialDataContainer::find(&mut empty, &10), None);
        assert_eq!(
            SerialDataContainer::find_index_with_context(
                &mut empty,
                &10,
                &mut SearchContext::none()
            ),
            SearchIndexes::none()
        );

        let mut one = vec![10];
        assert_eq!(
            SerialDataContainer::find_index(&mut one, &5),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0),
            }
        );
        assert_eq!(
            SerialDataContainer::find_index(&mut one, &10),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: None,
            }
        );
        assert_eq!(
            SerialDataContainer::find_index(&mut one, &15),
            SearchIndexes {
                before: Some(0),
                exact: None,
                after: None,
            }
        );
    }

    #[test]
    fn serial_data_container_auto_insert_keeps_values_sorted() {
        let mut values = Vec::new();

        SerialDataContainer::auto_insert(&mut values, 30);
        SerialDataContainer::auto_insert(&mut values, 10);
        SerialDataContainer::auto_insert(&mut values, 20);
        SerialDataContainer::auto_insert(&mut values, 20);
        SerialDataContainer::auto_insert(&mut values, 5);
        SerialDataContainer::auto_insert(&mut values, 40);

        assert_eq!(values, [5, 10, 20, 30, 40]);

        #[derive(Debug)]
        struct Entry {
            key: i32,
            value: &'static str,
        }

        impl PartialEq for Entry {
            fn eq(&self, other: &Self) -> bool {
                self.key == other.key
            }
        }

        impl PartialOrd for Entry {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                self.key.partial_cmp(&other.key)
            }
        }

        let mut entries = vec![
            Entry {
                key: 1,
                value: "one",
            },
            Entry {
                key: 3,
                value: "old",
            },
        ];
        SerialDataContainer::auto_insert(
            &mut entries,
            Entry {
                key: 3,
                value: "updated",
            },
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].value, "updated");
    }
}
