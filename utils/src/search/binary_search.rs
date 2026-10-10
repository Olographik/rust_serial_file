use std::{cmp::Ordering, fmt::Debug};

// Enum With indication of the Search result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchIndex {
    None,          // Target not found
    Before(usize), // Target is greater than found
    Exact(usize),  // Target is exact
    After(usize),  // Target is lower than found
}

pub struct ItemIndex<T> {
    pub value: T,
    pub index: usize,
}

impl SearchIndex {
    pub fn get<'a, 'b, T>(&'a self, array: &'b [T]) -> Option<&'b T> {
        match self {
            SearchIndex::None => None,
            SearchIndex::Before(idx) => Some(&array[*idx]),
            SearchIndex::Exact(idx) => Some(&array[*idx]),
            SearchIndex::After(idx) => Some(&array[*idx]),
        }
    }
    pub fn get_mut<'a, 'b, T>(&'a self, array: &'b mut [T]) -> Option<&'b mut T> {
        match self {
            SearchIndex::None => None,
            SearchIndex::Before(idx) => Some(&mut array[*idx]),
            SearchIndex::Exact(idx) => Some(&mut array[*idx]),
            SearchIndex::After(idx) => Some(&mut array[*idx]),
        }
    }
    pub fn get_index<'a, 'b, T>(&'a self, array: &'b [T]) -> Option<ItemIndex<&'b T>> {
        match self {
            SearchIndex::None => None,
            SearchIndex::Before(idx) => Some(ItemIndex {
                value: &array[*idx],
                index: idx.clone(),
            }),
            SearchIndex::Exact(idx) => Some(ItemIndex {
                value: &array[*idx],
                index: idx.clone(),
            }),
            SearchIndex::After(idx) => Some(ItemIndex {
                value: &array[*idx],
                index: idx.clone(),
            }),
        }
    }
    pub fn get_mut_index<'a, 'b, T>(&'a self, array: &'b mut [T]) -> Option<ItemIndex<&'b mut T>> {
        match self {
            SearchIndex::None => None,
            SearchIndex::Before(idx) => Some(ItemIndex {
                value: &mut array[*idx],
                index: idx.clone(),
            }),
            SearchIndex::Exact(idx) => Some(ItemIndex {
                value: &mut array[*idx],
                index: idx.clone(),
            }),
            SearchIndex::After(idx) => Some(ItemIndex {
                value: &mut array[*idx],
                index: idx.clone(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchQuery {
    Any,           // Return Before, Exact, After
    BeforeOnly,    // Return Before only
    BeforeOrExact, // Return Before or exact
    Exact,         // Return Exact only
    AfterOnly,     // return After only
    AfterOrExact,  // return After or exact
}

impl SearchIndex {
    pub fn from_query_value(
        query: &SearchQuery,
        comp_result: Ordering,
        index: usize,
        max_size: usize,
    ) -> Self {
        match query {
            // Any kind work -> qualify search result
            SearchQuery::Any => {
                match comp_result {
                    Ordering::Less => {
                        // target greater
                        SearchIndex::Before(index)
                    }
                    Ordering::Greater => {
                        // target lower
                        SearchIndex::After(index)
                    }
                    Ordering::Equal => SearchIndex::Exact(index),
                }
            }
            SearchQuery::BeforeOnly => {
                // Only for those before
                match comp_result {
                    Ordering::Less => {
                        // target already greater
                        SearchIndex::Before(index)
                    }
                    _ => {
                        if index == 0 {
                            // cannot go under 0 -> none match query
                            SearchIndex::None
                        } else {
                            // if target == array_value -> need before
                            // if target < arrray_value -> exact not found, before is before target
                            SearchIndex::Before(index - 1)
                        }
                    }
                }
            }
            SearchQuery::BeforeOrExact => {
                // For those before or exact
                match comp_result {
                    Ordering::Less => {
                        // target already greater
                        SearchIndex::Before(index)
                    }
                    Ordering::Equal => SearchIndex::Exact(index),
                    Ordering::Greater => {
                        if index == 0 {
                            // cannot go under 0 -> none match query
                            SearchIndex::None
                        } else {
                            // if target < arrray_value -> exact not found, before is before target
                            SearchIndex::Before(index - 1)
                        }
                    }
                }
            }
            SearchQuery::Exact => {
                // Only exact value
                if comp_result == Ordering::Equal {
                    SearchIndex::Exact(index)
                } else {
                    SearchIndex::None
                }
            }
            SearchQuery::AfterOnly => {
                //Only for those after
                if comp_result == Ordering::Greater {
                    // target already lower
                    SearchIndex::After(index)
                } else if index >= max_size - 1 {
                    // there is no value strictly after a target beyond the end of the array
                    SearchIndex::None
                } else {
                    // if target == array_value -> need after
                    // if target > array_value -> exact not found, after is after target
                    SearchIndex::After(index + 1)
                }
            }
            SearchQuery::AfterOrExact => {
                //For those after or exact
                match comp_result {
                    Ordering::Less => {
                        if index >= max_size - 1 {
                            // there is no value strictly after a target beyond the end of the array
                            SearchIndex::None
                        } else {
                            // if target > array_value -> exact not found, after is after target
                            SearchIndex::After(index + 1)
                        }
                    }
                    Ordering::Equal => SearchIndex::Exact(index),
                    Ordering::Greater => {
                        // target already lower
                        SearchIndex::After(index)
                    }
                }
            }
        }
    }
}

/// Comparator: comparator(index) -> ordering
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_dicotomic_search<F>(
    mut comparator: F,
    size: usize,
    query: SearchQuery,
) -> SearchIndex
where
    F: FnMut(usize) -> Ordering,
{
    if size == 0 {
        return SearchIndex::None;
    }

    let mut lower = 0usize;
    let mut upper = size;

    // check if inside the array first
    let low_comp = comparator(lower);
    if low_comp == Ordering::Greater {
        return SearchIndex::from_query_value(&query, low_comp, lower, size);
    } else {
        let upper_comp = comparator(upper - 1);
        if upper_comp == Ordering::Less {
            return SearchIndex::from_query_value(&query, upper_comp, upper - 1, size);
        }
    }

    while lower < upper {
        let mid = lower + (upper - lower) / 2;
        let mid_comp = comparator(mid);
        match mid_comp {
            Ordering::Less => lower = mid + 1,
            _ => upper = mid,
        }
    }

    let value = comparator(lower);
    // already bound check, lower E [ 0, array.len() [
    SearchIndex::from_query_value(&query, value, lower, size)
}

pub fn dicotomic_search_fn_mut<'a, F, T, K>(
    mut value_getter: F,
    target: &K,
    size: usize,
    query: SearchQuery,
) -> SearchIndex
where
    F: FnMut(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    comparator_dicotomic_search(
        |i| value_getter(i).partial_cmp(target).unwrap(),
        size,
        query,
    )
}

pub fn dicotomic_search_fn<'a, F, T, K>(
    value_getter: F,
    target: &K,
    size: usize,
    query: SearchQuery,
) -> SearchIndex
where
    F: Fn(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    dicotomic_search_fn_mut(value_getter, target, size, query)
}

pub fn dicotomic_search<T, K>(array: &[T], target: &K, query: SearchQuery) -> SearchIndex
where
    T: PartialOrd<K>,
{
    return dicotomic_search_fn(|i| &array[i], target, array.len(), query);
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::{SearchIndex, SearchQuery, dicotomic_search};

    #[test]
    fn exact_match_returns_index() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &5, SearchQuery::Exact),
            SearchIndex::Exact(2)
        );
    }

    #[test]
    fn missing_value_uses_lower_bound_for_before_query() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::BeforeOnly),
            SearchIndex::Before(2)
        );
    }

    #[test]
    fn missing_value_uses_upper_bound_for_after_query() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::AfterOnly),
            SearchIndex::After(3)
        );
    }

    #[test]
    fn empty_array_returns_none() {
        let values: Vec<i32> = vec![];
        assert_eq!(
            dicotomic_search(&values, &42, SearchQuery::Any),
            SearchIndex::None
        );
    }

    #[test]
    fn target_below_first_value_returns_after_index_zero() {
        let values = vec![2, 4, 6, 8, 10];
        assert_eq!(
            dicotomic_search(&values, &1, SearchQuery::Any),
            SearchIndex::After(0)
        );
        assert_eq!(
            dicotomic_search(&values, &1, SearchQuery::AfterOnly),
            SearchIndex::After(0)
        );
        assert_eq!(
            dicotomic_search(&values, &1, SearchQuery::BeforeOnly),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &1, SearchQuery::Exact),
            SearchIndex::None
        );
    }

    #[test]
    fn target_above_last_value_returns_before_last_index() {
        let values = vec![2, 4, 6, 8, 10];
        assert_eq!(
            dicotomic_search(&values, &11, SearchQuery::Any),
            SearchIndex::Before(4)
        );
        assert_eq!(
            dicotomic_search(&values, &11, SearchQuery::BeforeOnly),
            SearchIndex::Before(4)
        );
        assert_eq!(
            dicotomic_search(&values, &11, SearchQuery::AfterOnly),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &11, SearchQuery::Exact),
            SearchIndex::None
        );
    }

    #[test]
    fn exact_match_at_beginning_and_end_are_returned() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &1, SearchQuery::Any),
            SearchIndex::Exact(0)
        );
        assert_eq!(
            dicotomic_search(&values, &9, SearchQuery::Any),
            SearchIndex::Exact(4)
        );
    }

    #[test]
    fn any_query_uses_exact_when_present() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &5, SearchQuery::Any),
            SearchIndex::Exact(2)
        );
    }

    struct TestStruc(pub i64);
    impl Deref for TestStruc {
        type Target = i64;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl PartialEq<i32> for TestStruc {
        fn eq(&self, other: &i32) -> bool {
            self.0 == *other as i64
        }
    }

    impl PartialOrd<i32> for TestStruc {
        fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
            let as_i64 = *other as i64;
            self.0.partial_cmp(&as_i64)
        }
    }

    #[test]
    fn mixed_key_types_are_supported() {
        let values = vec![
            TestStruc(1),
            TestStruc(3),
            TestStruc(5),
            TestStruc(7),
            TestStruc(9),
        ];

        assert_eq!(
            dicotomic_search(&values, &5_i32, SearchQuery::Exact),
            SearchIndex::Exact(2)
        );
        assert_eq!(
            dicotomic_search(&values, &6_i32, SearchQuery::BeforeOnly),
            SearchIndex::Before(2)
        );
        assert_eq!(
            dicotomic_search(&values, &6_i32, SearchQuery::AfterOnly),
            SearchIndex::After(3)
        );
        assert_eq!(
            dicotomic_search(&values, &0_i32, SearchQuery::Any),
            SearchIndex::After(0)
        );
        assert_eq!(
            dicotomic_search(&values, &10_i32, SearchQuery::Any),
            SearchIndex::Before(4)
        );
    }

    #[test]
    fn inclusive_query_modes_cover_exact_and_boundary_cases() {
        let values = vec![1, 3, 5, 7, 9];

        assert_eq!(
            dicotomic_search(&values, &5, SearchQuery::BeforeOrExact),
            SearchIndex::Exact(2)
        );
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::BeforeOrExact),
            SearchIndex::Before(2)
        );
        assert_eq!(
            dicotomic_search(&values, &0, SearchQuery::BeforeOrExact),
            SearchIndex::None
        );

        assert_eq!(
            dicotomic_search(&values, &5, SearchQuery::AfterOrExact),
            SearchIndex::Exact(2)
        );
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::AfterOrExact),
            SearchIndex::After(3)
        );
        assert_eq!(
            dicotomic_search(&values, &10, SearchQuery::AfterOrExact),
            SearchIndex::None
        );
    }

    #[test]
    fn search_index_accessors_cover_valid_and_none_cases() {
        let mut values = vec![10, 20, 30, 40];

        assert_eq!(SearchIndex::Exact(2).get(&values), Some(&30));
        assert_eq!(SearchIndex::Before(1).get(&values), Some(&20));
        assert_eq!(SearchIndex::After(1).get(&values), Some(&20));
        assert_eq!(SearchIndex::None.get(&values), None);

        if let Some(value) = SearchIndex::Exact(2).get_mut(&mut values) {
            *value = 99;
        }
        assert_eq!(values, vec![10, 20, 99, 40]);

        assert_eq!(SearchIndex::None.get_mut(&mut values), None);
    }

    #[test]
    fn single_element_cases_cover_all_query_modes() {
        let values = vec![7];

        assert_eq!(
            dicotomic_search(&values, &7, SearchQuery::Exact),
            SearchIndex::Exact(0)
        );
        assert_eq!(
            dicotomic_search(&values, &7, SearchQuery::Any),
            SearchIndex::Exact(0)
        );
        assert_eq!(
            dicotomic_search(&values, &7, SearchQuery::BeforeOnly),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &7, SearchQuery::AfterOnly),
            SearchIndex::None
        );

        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::Exact),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::Any),
            SearchIndex::After(0)
        );
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::BeforeOnly),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &6, SearchQuery::AfterOnly),
            SearchIndex::After(0)
        );

        assert_eq!(
            dicotomic_search(&values, &8, SearchQuery::Exact),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &8, SearchQuery::Any),
            SearchIndex::Before(0)
        );
        assert_eq!(
            dicotomic_search(&values, &8, SearchQuery::BeforeOnly),
            SearchIndex::Before(0)
        );
        assert_eq!(
            dicotomic_search(&values, &8, SearchQuery::AfterOnly),
            SearchIndex::None
        );
    }

    #[test]
    fn duplicate_values_still_return_the_leftmost_exact_position() {
        let values = vec![1, 2, 2, 2, 3];

        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::Exact),
            SearchIndex::Exact(1)
        );
        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::Any),
            SearchIndex::Exact(1)
        );
        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::BeforeOnly),
            SearchIndex::Before(0)
        );
        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::AfterOnly),
            SearchIndex::After(2)
        );
    }

    #[test]
    fn first_and_last_values_have_correct_before_after_behaviour() {
        let values = vec![2, 4, 6, 8, 10];

        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::BeforeOnly),
            SearchIndex::None
        );
        assert_eq!(
            dicotomic_search(&values, &2, SearchQuery::AfterOnly),
            SearchIndex::After(1)
        );

        assert_eq!(
            dicotomic_search(&values, &10, SearchQuery::BeforeOnly),
            SearchIndex::Before(3)
        );
        assert_eq!(
            dicotomic_search(&values, &10, SearchQuery::AfterOnly),
            SearchIndex::None
        );
    }
}
