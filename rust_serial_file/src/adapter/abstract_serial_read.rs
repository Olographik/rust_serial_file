use std::{cmp::Ordering, error::Error, fmt};

use anyhow::{Result, anyhow};
use utils::search::search_type::{SearchContext, SearchIndexes};

/// Errors produced by the built-in `SerialDataReader` operations.
#[derive(Debug, PartialEq, Eq)]
pub enum SerialDataReaderError {
    IndexOutOfBounds { index: usize, len: usize },
    EmptyCollection,
    UndefinedOrdering,
}

impl fmt::Display for SerialDataReaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexOutOfBounds { index, len } => {
                write!(
                    formatter,
                    "index {index} is out of bounds for collection of length {len}"
                )
            }
            Self::EmptyCollection => write!(formatter, "collection is empty"),
            Self::UndefinedOrdering => write!(formatter, "values do not have a defined ordering"),
        }
    }
}

impl Error for SerialDataReaderError {}

/// Reads owned records from a logically ordered serial collection.
///
/// Implementations may update internal state while reading, for example by
/// refilling a file buffer or advancing a cursor. Those updates must not alter
/// the logical records or their order. Operations that access backend data
/// return [`anyhow::Result`] so storage and decoding errors remain observable.
///
/// Searches use binary search and therefore require records to be ordered
/// according to the comparison used by the search. They do not sort or mutate
/// the collection.
pub trait SerialDataReader<T> {
    /// Iterator over owned records in logical collection order.
    ///
    /// Errors encountered while reading individual records are yielded as
    /// `Err` items.
    type Iter<'a>: Iterator<Item = Result<T>>
    where
        Self: 'a,
        T: 'a;

    /// Returns the logical number of records.
    ///
    /// Implementations must make this metadata available without fallible
    /// backend I/O.
    fn serial_len(&self) -> usize;

    /// Returns whether the logical collection contains no records.
    fn serial_is_empty(&self) -> bool {
        self.serial_len() == 0
    }

    /// Reads the record at the zero-based `index`.
    ///
    /// Returns an error if the index is out of bounds or if the backend cannot
    /// read or decode the record.
    fn serial_get(&mut self, index: usize) -> Result<T>;

    /// Reads the first record.
    ///
    /// Returns [`SerialDataReaderError::EmptyCollection`] when the collection
    /// is empty, or propagates a backend read error.
    fn serial_first(&mut self) -> Result<T> {
        if self.serial_is_empty() {
            return Err(SerialDataReaderError::EmptyCollection.into());
        }
        self.serial_get(0)
    }

    /// Reads the last record.
    ///
    /// Returns [`SerialDataReaderError::EmptyCollection`] when the collection
    /// is empty, or propagates a backend read error.
    fn serial_last(&mut self) -> Result<T> {
        let len = self.serial_len();
        if len == 0 {
            return Err(SerialDataReaderError::EmptyCollection.into());
        }
        self.serial_get(len - 1)
    }

    /// Creates an iterator that reads owned records in logical collection
    /// order.
    ///
    /// Iterator creation may fail, and failures encountered during iteration
    /// are yielded as `Err` items.
    fn serial_iter(&mut self) -> Result<Self::Iter<'_>>;

    /// Finds a value using its `PartialOrd` ordering.
    ///
    /// Returns the index of a matching record, or `None` if no record matches.
    /// Returns an error if a read fails or a comparison has no defined
    /// ordering.
    fn serial_find(&mut self, value: &T) -> Result<Option<usize>>
    where
        T: PartialOrd<T>,
    {
        self.serial_find_by(value)
    }

    /// Finds a record by comparing it with `key`.
    ///
    /// `T` may be partially ordered against a key type different from `T`.
    /// The collection must be ordered according to this comparison. Returns
    /// the matching index, `None` if no record matches, or an error if a read
    /// fails or the comparison has no defined ordering.
    fn serial_find_by<K>(&mut self, key: &K) -> Result<Option<usize>>
    where
        T: PartialOrd<K>,
    {
        let len = self.serial_len();
        let search = fallible_dicotomic_search(len, None, |index| {
            let record = self.serial_get(index)?;
            record
                .partial_cmp(key)
                .ok_or_else(|| SerialDataReaderError::UndefinedOrdering.into())
        })?;
        Ok(search.exact)
    }

    /// Finds a record using a comparator against a captured search target.
    ///
    /// The comparator receives each visited record and returns its ordering
    /// relative to the target (`Less` before it, `Equal` at it, and `Greater`
    /// after it). Records must be ordered consistently with that comparison.
    /// Returns the matching index, `None` if no record matches, or a backend
    /// read error.
    fn serial_find_by_ordering<F>(&mut self, mut comparator: F) -> Result<Option<usize>>
    where
        F: FnMut(&T) -> Ordering,
    {
        let len = self.serial_len();
        let search = fallible_dicotomic_search(len, None, |index| {
            let record = self.serial_get(index)?;
            Ok(comparator(&record))
        })?;
        Ok(search.exact)
    }

    /// Finds a value using its `PartialOrd` ordering and a search context.
    ///
    /// The context may seed the search and is updated with the resulting
    /// search position. Returns the matching index, `None` if no record
    /// matches, or an error if a read fails or a comparison has no defined
    /// ordering.
    fn serial_find_with_context(
        &mut self,
        value: &T,
        context: &mut SearchContext,
    ) -> Result<Option<usize>>
    where
        T: PartialOrd<T>,
    {
        self.serial_find_by_with_context(value, context)
    }

    /// Finds a record by comparing it with `key`, using a search context.
    ///
    /// The context may seed the search and is updated with the resulting
    /// search position. The collection must be ordered according to
    /// `T: PartialOrd<K>`. Returns the matching index, `None` if no record
    /// matches, or an error if a read fails or a comparison has no defined
    /// ordering.
    fn serial_find_by_with_context<K>(
        &mut self,
        key: &K,
        context: &mut SearchContext,
    ) -> Result<Option<usize>>
    where
        T: PartialOrd<K>,
    {
        let len = self.serial_len();
        let search = fallible_dicotomic_search(len, Some(context), |index| {
            let record = self.serial_get(index)?;
            record
                .partial_cmp(key)
                .ok_or_else(|| SerialDataReaderError::UndefinedOrdering.into())
        })?;
        Ok(search.exact)
    }

    /// Finds a record using a comparator and a search context.
    ///
    /// The comparator returns each visited record's ordering relative to a
    /// captured search target. The context may seed the search and is updated
    /// with the resulting search position. Records must be ordered consistently
    /// with the comparator. Returns the matching index, `None` if no record
    /// matches, or a backend read error.
    fn serial_find_by_ordering_with_context<F>(
        &mut self,
        mut comparator: F,
        context: &mut SearchContext,
    ) -> Result<Option<usize>>
    where
        F: FnMut(&T) -> Ordering,
    {
        let len = self.serial_len();
        let search = fallible_dicotomic_search(len, Some(context), |index| {
            let record = self.serial_get(index)?;
            Ok(comparator(&record))
        })?;
        Ok(search.exact)
    }

    /// Finds a value and reads the matching record.
    ///
    /// Returns `Ok(Some(record))` when a matching record is found,
    /// `Ok(None)` when there is no match, or `Err` if searching or reading the
    /// record fails.
    fn serial_find_and_get(&mut self, value: &T) -> Result<Option<T>>
    where
        T: PartialOrd<T>,
    {
        self.serial_find(value)?
            .map(|index| self.serial_get(index))
            .transpose()
    }

    /// Finds a record by key and reads the matching record.
    ///
    /// Returns `Ok(Some(record))` when a matching record is found,
    /// `Ok(None)` when there is no match, or `Err` if searching or reading the
    /// record fails.
    fn serial_find_by_and_get<K>(&mut self, key: &K) -> Result<Option<T>>
    where
        T: PartialOrd<K>,
    {
        self.serial_find_by(key)?
            .map(|index| self.serial_get(index))
            .transpose()
    }
}

fn fallible_dicotomic_search<F>(
    size: usize,
    mut context: Option<&mut SearchContext>,
    mut compare_at: F,
) -> Result<SearchIndexes>
where
    F: FnMut(usize) -> Result<Ordering>,
{
    if size == 0 {
        return Ok(SearchIndexes::none());
    }

    let mut lower = 0;
    let mut upper = size;

    if let Some(search_context) = context.as_deref_mut()
        && let Some(index) = search_context.0.filter(|index| *index < size)
    {
        match compare_at(index)? {
            Ordering::Less => lower = index + 1,
            Ordering::Equal => return Ok(SearchIndexes::from_exact_point(index, size)),
            Ordering::Greater => upper = index,
        }
    }

    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        match compare_at(middle)? {
            Ordering::Less => lower = middle + 1,
            Ordering::Equal | Ordering::Greater => upper = middle,
        }
    }

    let result = if lower == size {
        SearchIndexes::from_before_point(size - 1, size)
    } else {
        match compare_at(lower)? {
            Ordering::Equal => SearchIndexes::from_exact_point(lower, size),
            Ordering::Less => {
                return Err(anyhow!(
                    "binary-search comparator returned inconsistent ordering"
                ));
            }
            Ordering::Greater if lower == 0 => SearchIndexes::from_after_point(0),
            Ordering::Greater => SearchIndexes {
                before: Some(lower - 1),
                exact: None,
                after: Some(lower),
            },
        }
    };

    if let Some(search_context) = context {
        search_context.update_context(&result);
    }

    Ok(result)
}

fn clone_to_result<T: Clone>(value: &T) -> Result<T> {
    Ok(value.clone())
}

impl<T: Clone> SerialDataReader<T> for Vec<T> {
    type Iter<'a>
        = std::iter::Map<std::slice::Iter<'a, T>, fn(&'a T) -> Result<T>>
    where
        T: 'a;

    fn serial_len(&self) -> usize {
        Vec::len(self)
    }

    fn serial_get(&mut self, index: usize) -> Result<T> {
        self.as_slice().get(index).cloned().ok_or_else(|| {
            SerialDataReaderError::IndexOutOfBounds {
                index,
                len: Vec::len(self),
            }
            .into()
        })
    }

    fn serial_iter(&mut self) -> Result<Self::Iter<'_>> {
        Ok(self.iter().map(clone_to_result::<T>))
    }
}

#[cfg(test)]
mod tests {
    use std::{cmp::Ordering, collections::BTreeMap, error::Error};

    use super::{SerialDataReader, SerialDataReaderError};
    use anyhow::{Result, anyhow};
    use utils::search::search_type::SearchContext;

    struct BufferedReader {
        records: Vec<i32>,
        buffer: Vec<i32>,
        buffer_start: usize,
        buffer_capacity: usize,
        read_count: usize,
        fail_read_at: Option<usize>,
        fail_read_on_call: Option<usize>,
        fail_iterator_creation: bool,
        iterator_error_at: Option<usize>,
    }

    impl BufferedReader {
        fn new(records: Vec<i32>, buffer_capacity: usize) -> Self {
            Self {
                records,
                buffer: Vec::new(),
                buffer_start: 0,
                buffer_capacity,
                read_count: 0,
                fail_read_at: None,
                fail_read_on_call: None,
                fail_iterator_creation: false,
                iterator_error_at: None,
            }
        }
    }

    struct BufferedIter<'a> {
        reader: &'a mut BufferedReader,
        index: usize,
        len: usize,
    }

    impl Iterator for BufferedIter<'_> {
        type Item = Result<i32>;

        fn next(&mut self) -> Option<Self::Item> {
            if self.index == self.len {
                return None;
            }

            let index = self.index;
            self.index += 1;
            if self.reader.iterator_error_at == Some(index) {
                return Some(Err(anyhow!("injected iteration error at index {index}")));
            }
            Some(self.reader.serial_get(index))
        }
    }

    impl SerialDataReader<i32> for BufferedReader {
        type Iter<'a> = BufferedIter<'a>;

        fn serial_len(&self) -> usize {
            self.records.len()
        }

        fn serial_get(&mut self, index: usize) -> Result<i32> {
            let read_number = self.read_count;
            self.read_count += 1;
            if self.fail_read_at == Some(index) {
                return Err(anyhow!("injected read error at index {index}"));
            }
            if self.fail_read_on_call == Some(read_number) {
                return Err(anyhow!("injected read error on call {read_number}"));
            }

            if index >= self.records.len() {
                return Err(SerialDataReaderError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into());
            }

            let buffer_end = self.buffer_start + self.buffer.len();
            if index < self.buffer_start || index >= buffer_end {
                self.buffer_start = (index / self.buffer_capacity) * self.buffer_capacity;
                let end = (self.buffer_start + self.buffer_capacity).min(self.records.len());
                self.buffer = self.records[self.buffer_start..end].to_vec();
            }

            Ok(self.buffer[index - self.buffer_start])
        }

        fn serial_iter(&mut self) -> Result<Self::Iter<'_>> {
            if self.fail_iterator_creation {
                return Err(anyhow!("injected iterator creation error"));
            }

            let len = self.serial_len();
            Ok(BufferedIter {
                reader: self,
                index: 0,
                len,
            })
        }
    }

    struct MapReader {
        records: BTreeMap<usize, i32>,
        fail_iterator_creation: bool,
    }

    struct MapIter<'a> {
        records: std::collections::btree_map::Values<'a, usize, i32>,
    }

    impl Iterator for MapIter<'_> {
        type Item = Result<i32>;

        fn next(&mut self) -> Option<Self::Item> {
            self.records.next().copied().map(Ok)
        }
    }

    impl SerialDataReader<i32> for MapReader {
        type Iter<'a> = MapIter<'a>;

        fn serial_len(&self) -> usize {
            self.records.len()
        }

        fn serial_get(&mut self, index: usize) -> Result<i32> {
            self.records.get(&index).copied().ok_or_else(|| {
                SerialDataReaderError::IndexOutOfBounds {
                    index,
                    len: self.records.len(),
                }
                .into()
            })
        }

        fn serial_iter(&mut self) -> Result<Self::Iter<'_>> {
            if self.fail_iterator_creation {
                return Err(anyhow!("injected map iterator creation error"));
            }
            Ok(MapIter {
                records: self.records.values(),
            })
        }
    }

    fn read_all<R: SerialDataReader<i32>>(reader: &mut R) -> Result<Vec<i32>> {
        reader.serial_iter()?.collect()
    }

    #[test]
    fn vector_reader_returns_owned_records_and_reports_invalid_indices() {
        let mut values = vec![10, 20, 30];

        assert_eq!(values.serial_len(), 3);
        assert!(!values.serial_is_empty());
        assert_eq!(values.serial_get(1).unwrap(), 20);
        assert_eq!(values.serial_first().unwrap(), 10);
        assert_eq!(values.serial_last().unwrap(), 30);
        assert_eq!(
            values
                .serial_get(3)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 3, len: 3 })
        );
        assert_eq!(
            values
                .serial_iter()
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [10, 20, 30]
        );

        let mut empty: Vec<i32> = Vec::new();
        assert!(empty.serial_is_empty());
        assert_eq!(
            empty
                .serial_first()
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::EmptyCollection)
        );
        assert_eq!(
            empty
                .serial_last()
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::EmptyCollection)
        );
        assert_eq!(empty.serial_find_by(&10).unwrap(), None);
    }

    #[test]
    fn reader_errors_have_display_messages_and_no_sources() {
        let errors = [
            (
                SerialDataReaderError::IndexOutOfBounds { index: 4, len: 2 },
                "index 4 is out of bounds for collection of length 2",
            ),
            (
                SerialDataReaderError::EmptyCollection,
                "collection is empty",
            ),
            (
                SerialDataReaderError::UndefinedOrdering,
                "values do not have a defined ordering",
            ),
        ];

        for (error, message) in errors {
            assert_eq!(error.to_string(), message);
            assert!(Error::source(&error).is_none());
        }
    }

    #[test]
    fn reader_searches_by_value_key_and_comparator() {
        #[derive(Clone, Debug)]
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

        impl PartialOrd<i32> for Entry {
            fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
                self.key.partial_cmp(other)
            }
        }

        impl PartialEq<i32> for Entry {
            fn eq(&self, other: &i32) -> bool {
                self.key == *other
            }
        }

        let mut entries = vec![
            Entry {
                key: 10,
                value: "ten",
            },
            Entry {
                key: 20,
                value: "twenty",
            },
            Entry {
                key: 30,
                value: "thirty",
            },
        ];
        let target = Entry {
            key: 20,
            value: "query",
        };
        assert!(target == target.clone());
        assert!(entries[0] == 10);
        assert_eq!(entries.serial_find(&target).unwrap(), Some(1));
        assert_eq!(entries.serial_find_by(&30).unwrap(), Some(2));
        assert_eq!(
            entries
                .serial_find_by_ordering(|entry| entry.key.cmp(&10))
                .unwrap(),
            Some(0)
        );
        assert_eq!(entries.serial_find_by(&25).unwrap(), None);
        assert_eq!(
            entries.serial_find_by_and_get(&20).unwrap().unwrap().value,
            "twenty"
        );
        assert_eq!(
            entries.serial_find_and_get(&target).unwrap().unwrap().value,
            "twenty"
        );
        assert_eq!(
            entries
                .serial_find_and_get(&Entry {
                    key: 25,
                    value: "missing",
                })
                .unwrap(),
            None
        );
        assert_eq!(
            entries
                .serial_get(3)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 3, len: 3 })
        );

        let mut context = SearchContext::none();
        assert_eq!(
            entries
                .serial_find_by_with_context(&30, &mut context)
                .unwrap(),
            Some(2)
        );
        assert_eq!(context, SearchContext::of(2));

        let mut context = SearchContext::of(2);
        assert_eq!(
            entries
                .serial_find_with_context(&target, &mut context)
                .unwrap(),
            Some(1)
        );
        assert_eq!(context, SearchContext::of(1));

        let mut context = SearchContext::of(0);
        assert_eq!(
            entries
                .serial_find_by_with_context(&25, &mut context)
                .unwrap(),
            None
        );
        assert_eq!(context, SearchContext::of(1));

        let mut context = SearchContext::of(0);
        assert_eq!(
            entries
                .serial_find_by_ordering_with_context(|entry| entry.key.cmp(&30), &mut context)
                .unwrap(),
            Some(2)
        );
        assert_eq!(context, SearchContext::of(2));
    }

    #[test]
    fn searches_cover_empty_boundaries_context_seeds_and_inconsistent_comparators() {
        let mut empty: Vec<i32> = Vec::new();
        let mut context = SearchContext::none();
        assert_eq!(
            empty
                .serial_find_by_with_context(&10, &mut context)
                .unwrap(),
            None
        );
        let comparator = |record: &i32| record.cmp(&10);
        assert_eq!(
            empty
                .serial_find_by_ordering_with_context(comparator, &mut context)
                .unwrap(),
            None
        );
        let mut non_empty = vec![10, 20, 30];
        let mut context = SearchContext::none();
        assert_eq!(
            non_empty
                .serial_find_by_ordering_with_context(comparator, &mut context)
                .unwrap(),
            Some(0)
        );

        let mut values = vec![10, 20, 30];
        assert_eq!(values.serial_find_by(&5).unwrap(), None);
        assert_eq!(values.serial_find_by(&40).unwrap(), None);

        let mut context = SearchContext::of(0);
        assert_eq!(
            values
                .serial_find_by_with_context(&30, &mut context)
                .unwrap(),
            Some(2)
        );
        assert_eq!(context, SearchContext::of(2));

        let mut context = SearchContext::of(2);
        assert_eq!(
            values
                .serial_find_by_ordering_with_context(|record| record.cmp(&10), &mut context)
                .unwrap(),
            Some(0)
        );
        assert_eq!(context, SearchContext::of(0));

        let mut context = SearchContext::of(1);
        assert_eq!(
            values
                .serial_find_by_ordering_with_context(|record| record.cmp(&20), &mut context)
                .unwrap(),
            Some(1)
        );

        let mut compared_middle = false;
        let error = values
            .serial_find_by_ordering(|record| match *record {
                20 if !compared_middle => {
                    compared_middle = true;
                    Ordering::Greater
                }
                20 => Ordering::Less,
                _ => Ordering::Less,
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("binary-search comparator returned inconsistent ordering")
        );

        let mut reader = BufferedReader::new(vec![10, 20, 30], 2);
        reader.fail_read_on_call = Some(2);
        assert!(
            reader
                .serial_find_by(&15)
                .unwrap_err()
                .to_string()
                .contains("read error on call")
        );

        assert_eq!(
            values
                .serial_find_by_ordering(|record| {
                    if *record == 20 {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                })
                .unwrap(),
            None
        );
    }

    #[test]
    fn search_methods_propagate_backend_and_comparison_errors() {
        let mut reader = BufferedReader::new(vec![10, 20, 30], 2);
        reader.fail_read_at = Some(1);

        let comparator = |record: &i32| record.cmp(&20);
        assert!(
            reader
                .serial_find_by_ordering(comparator)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;
        assert_eq!(reader.serial_find_by_ordering(comparator).unwrap(), Some(1));
        reader.fail_read_at = Some(1);

        let key = 20;
        let mut context = SearchContext::of(1);
        assert!(
            reader
                .serial_find_by_with_context(&key, &mut context)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;
        let mut context = SearchContext::of(1);
        assert_eq!(
            reader
                .serial_find_by_with_context(&key, &mut context)
                .unwrap(),
            Some(1)
        );
        reader.fail_read_at = Some(1);

        let comparator = |record: &i32| record.cmp(&20);
        let mut context = SearchContext::of(1);
        assert!(
            reader
                .serial_find_by_ordering_with_context(comparator, &mut context)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;
        let mut context = SearchContext::of(1);
        assert_eq!(
            reader
                .serial_find_by_ordering_with_context(comparator, &mut context)
                .unwrap(),
            Some(1)
        );
        reader.fail_read_at = Some(1);

        assert!(
            reader
                .serial_find_and_get(&20)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;
        assert_eq!(reader.serial_find_and_get(&20).unwrap(), Some(20));

        reader.fail_read_at = Some(1);
        assert!(
            reader
                .serial_find_by_and_get(&20)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;
        assert_eq!(reader.serial_find_by_and_get(&20).unwrap(), Some(20));

        let mut unordered = vec![1.0_f64, f64::NAN, 3.0];
        let mut context = SearchContext::none();
        assert_eq!(
            unordered
                .serial_find_by_with_context(&2.0, &mut context)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::UndefinedOrdering)
        );
    }

    #[test]
    fn undefined_partial_ordering_is_reported() {
        let mut values = vec![1.0_f64, f64::NAN, 3.0];
        let error = values.serial_find_by(&2.0).unwrap_err();
        assert_eq!(
            error.downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::UndefinedOrdering)
        );

        let mut ordered = vec![1.0_f64, 2.0, 3.0];
        assert_eq!(ordered.serial_find_by(&2.0).unwrap(), Some(1));
        assert_eq!(ordered.serial_find_by(&0.0).unwrap(), None);
        assert_eq!(ordered.serial_find_by(&4.0).unwrap(), None);

        let mut context = SearchContext::none();
        assert_eq!(
            ordered
                .serial_find_by_with_context(&2.0, &mut context)
                .unwrap(),
            Some(1)
        );
        assert_eq!(context, SearchContext::of(1));
    }

    #[test]
    fn buffered_reads_preserve_records_across_refills_and_iteration() {
        let mut reader = BufferedReader::new(vec![10, 20, 30, 40, 50], 2);

        assert_eq!(reader.serial_get(3).unwrap(), 40);
        assert_eq!(reader.buffer_start, 2);
        assert_eq!(reader.serial_get(0).unwrap(), 10);
        assert_eq!(reader.buffer_start, 0);
        assert_eq!(reader.serial_get(4).unwrap(), 50);
        assert_eq!(reader.buffer_start, 4);
        assert_eq!(reader.serial_get(3).unwrap(), 40);

        let values = reader
            .serial_iter()
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(values, [10, 20, 30, 40, 50]);
        assert_eq!(read_all(&mut reader).unwrap(), [10, 20, 30, 40, 50]);
    }

    #[test]
    fn backend_read_and_iterator_errors_remain_observable() {
        let mut empty_reader = BufferedReader::new(Vec::new(), 2);
        assert_eq!(
            empty_reader
                .serial_first()
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::EmptyCollection)
        );
        assert_eq!(
            empty_reader
                .serial_last()
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::EmptyCollection)
        );

        let mut reader = BufferedReader::new(vec![10, 20, 30], 2);
        reader.fail_read_at = Some(1);
        assert!(
            reader
                .serial_get(1)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        assert!(
            reader
                .serial_find_by(&20)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );

        reader.fail_read_at = Some(0);
        assert!(
            reader
                .serial_first()
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = Some(2);
        assert!(
            reader
                .serial_last()
                .unwrap_err()
                .to_string()
                .contains("read error")
        );

        reader.fail_read_at = None;
        assert_eq!(
            reader
                .serial_get(3)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 3, len: 3 })
        );
        reader.fail_iterator_creation = true;
        assert!(
            reader
                .serial_iter()
                .err()
                .unwrap()
                .to_string()
                .contains("iterator creation error")
        );

        reader.fail_iterator_creation = false;
        reader.iterator_error_at = Some(1);
        let mut iter = reader.serial_iter().unwrap();
        assert_eq!(iter.next().unwrap().unwrap(), 10);
        assert!(
            iter.next()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("iteration error")
        );
        assert_eq!(iter.next().unwrap().unwrap(), 30);
        assert!(iter.next().is_none());
    }

    #[test]
    fn collecting_an_iterator_propagates_item_errors() {
        let mut reader = BufferedReader::new(vec![10, 20, 30], 2);
        reader.iterator_error_at = Some(1);

        let error = read_all(&mut reader).unwrap_err();

        assert!(error.to_string().contains("iteration error at index 1"));

        reader.fail_iterator_creation = true;
        let error = read_all(&mut reader).unwrap_err();
        assert!(error.to_string().contains("iterator creation error"));
    }

    #[test]
    fn context_search_and_find_and_get_report_all_outcomes() {
        let mut reader = BufferedReader::new(vec![10, 20, 30], 2);
        let comparator = |record: &i32| record.cmp(&30);

        let mut context = SearchContext::none();
        assert_eq!(
            reader.serial_find_with_context(&20, &mut context).unwrap(),
            Some(1)
        );
        assert_eq!(context, SearchContext::of(1));

        let mut context = SearchContext::none();
        assert_eq!(
            reader
                .serial_find_by_ordering_with_context(comparator, &mut context,)
                .unwrap(),
            Some(2)
        );
        assert_eq!(context, SearchContext::of(2));

        reader.fail_read_at = Some(2);
        let mut context = SearchContext::none();
        assert!(
            reader
                .serial_find_by_ordering_with_context(comparator, &mut context)
                .unwrap_err()
                .to_string()
                .contains("read error")
        );
        reader.fail_read_at = None;

        assert_eq!(reader.serial_find_and_get(&20).unwrap(), Some(20));
        assert_eq!(reader.serial_find_by_and_get(&25).unwrap(), None);

        reader.fail_read_on_call = Some(reader.read_count + 3);
        let error = reader.serial_find_and_get(&20).unwrap_err();
        assert!(error.to_string().contains("read error"));
    }

    #[test]
    fn generic_read_client_supports_an_alternate_backend() {
        let mut reader = MapReader {
            records: [(0, 7), (1, 14), (2, 21)].into_iter().collect(),
            fail_iterator_creation: false,
        };

        assert_eq!(read_all(&mut reader).unwrap(), [7, 14, 21]);
        assert_eq!(reader.serial_get(1).unwrap(), 14);
        assert_eq!(reader.serial_find(&21).unwrap(), Some(2));
        assert_eq!(
            reader
                .serial_get(3)
                .unwrap_err()
                .downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 3, len: 3 })
        );

        reader.fail_iterator_creation = true;
        assert!(
            read_all(&mut reader)
                .unwrap_err()
                .to_string()
                .contains("map iterator creation error")
        );
    }

    #[test]
    fn alternate_backend_search_propagates_missing_record_error() {
        let mut reader = MapReader {
            records: [(0, 7), (2, 21)].into_iter().collect(),
            fail_iterator_creation: false,
        };

        let error = reader.serial_find_by(&14).unwrap_err();

        assert_eq!(
            error.downcast_ref::<SerialDataReaderError>(),
            Some(&SerialDataReaderError::IndexOutOfBounds { index: 1, len: 2 })
        );
    }
}
