use std::cmp::Ordering;

use crate::search::search_type::{SearchContext, SearchIndexes};

/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
fn comparator_linear_search_forward<F>(
    mut comparator: F,
    size: usize,
    start_point: usize,
) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    // equals mean none at this point
    let mut comp = Ordering::Equal;
    for i in start_point..size {
        comp = comparator(i);
        match comp {
            Ordering::Less => {
                //keep moving
            }
            Ordering::Equal => return SearchIndexes::from_exact_point(i, size),
            Ordering::Greater => {
                // we pass the point
                return SearchIndexes::from_after_point(i);
            }
        }
    }

    // if start point >= size, comp == Equal => none
    // only come to this call if context dispatcher see that we must go forward and we have room to go forward
    if comp == Ordering::Less {
        SearchIndexes {
            // we have been through the whole data (forward) only exist with less and last point is size-1
            before: Some(size - 1),
            exact: None,
            after: None,
        }
    } else {
        SearchIndexes::none()
    }
}

/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
fn comparator_linear_search_bacward<F>(
    mut comparator: F,
    size: usize,
    start_point: usize,
) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    // equals mean none at this point
    let mut comp = Ordering::Equal;
    for i in (0..=start_point).rev() {
        comp = comparator(i);
        match comp {
            Ordering::Less => {
                // we pass the point
                return SearchIndexes::from_before_point(i, size);
            }
            Ordering::Equal => return SearchIndexes::from_exact_point(i, size),
            Ordering::Greater => {
                //keep moving
            }
        }
    }

    // if start point == 0, comp == Equal => none
    // only come to this call if context dispatcher see that we must go backward and we have room to go forward
    if comp == Ordering::Greater {
        SearchIndexes {
            before: None,
            exact: None,
            after: Some(0),
        }
    } else {
        SearchIndexes::none()
    }
}
/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_linear_search_with_context<F>(
    mut comparator: F,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    if size == 0 {
        return SearchIndexes::none();
    }
    if size == 1 {
        return match comparator(0) {
            Ordering::Less => SearchIndexes {
                before: Some(0),
                exact: None,
                after: None,
            },
            Ordering::Equal => SearchIndexes {
                before: None,
                exact: Some(0),
                after: None,
            },
            Ordering::Greater => SearchIndexes {
                before: None,
                exact: None,
                after: Some(0),
            },
        };
    }

    match comparator(0) {
        Ordering::Less => {
            let upper = size - 1;
            match comparator(upper) {
                Ordering::Less => {
                    return SearchIndexes {
                        before: Some(upper),
                        exact: None,
                        after: None,
                    };
                }
                Ordering::Equal => return SearchIndexes::from_exact_point(upper, size),
                Ordering::Greater => {
                    //go through
                }
            }
        }
        Ordering::Equal => return SearchIndexes::from_exact_point(0, size),
        Ordering::Greater => {
            return SearchIndexes {
                before: None,
                exact: None,
                after: Some(0),
            };
        }
    }

    let search = if let Some(index) = context.0 {
        let comp = comparator(index);
        match comp {
            Ordering::Less => {
                // value is less than key, go forward
                let start_point = index + 1;
                if start_point < size {
                    // room to go forward, already check current, move one step
                    comparator_linear_search_forward(comparator, size, index + 1)
                } else {
                    // no room to go forward, exit
                    SearchIndexes {
                        before: Some(index),
                        exact: None,
                        after: None,
                    }
                }
            }
            Ordering::Equal => SearchIndexes::from_exact_point(index, size),
            Ordering::Greater => {
                // value is greater than key
                if index > 0 {
                    // room to go backward, already check current, move one step
                    comparator_linear_search_bacward(comparator, size, index - 1)
                } else {
                    // no room to go backward, exit
                    SearchIndexes {
                        before: None,
                        exact: None,
                        after: Some(index),
                    }
                }
            }
        }
    } else {
        comparator_linear_search_forward(comparator, size, 0)
    };
    context.update_context(&search);
    search
}

pub fn linear_search_with_context_fn_mut<'a, F, T, K>(
    mut value_getter: F,
    target: &K,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: FnMut(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    comparator_linear_search_with_context(
        |i| value_getter(i).partial_cmp(target).unwrap(),
        size,
        context,
    )
}

pub fn linear_search_with_context_fn<'a, F, T, K>(
    value_getter: F,
    target: &K,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: Fn(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    linear_search_with_context_fn_mut(value_getter, target, size, context)
}

pub fn linear_search_with_context<T, K>(
    array: &[T],
    target: &K,
    context: &mut SearchContext,
) -> SearchIndexes
where
    T: PartialOrd<K>,
{
    linear_search_with_context_fn(|i| &array[i], target, array.len(), context)
}

/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_linear_search<F>(comparator: F, size: usize) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    let mut context = SearchContext::none();
    comparator_linear_search_with_context(comparator, size, &mut context)
}

pub fn linear_search_fn_mut<'a, F, T, K>(
    mut value_getter: F,
    target: &K,
    size: usize,
) -> SearchIndexes
where
    F: FnMut(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    comparator_linear_search(|i| value_getter(i).partial_cmp(target).unwrap(), size)
}

pub fn linear_search_fn<'a, F, T, K>(value_getter: F, target: &K, size: usize) -> SearchIndexes
where
    F: Fn(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    linear_search_fn_mut(value_getter, target, size)
}

pub fn linear_search<T, K>(array: &[T], target: &K) -> SearchIndexes
where
    T: PartialOrd<K>,
{
    linear_search_fn(|i| &array[i], target, array.len())
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::search::{linear_search::linear_search, search_type::SearchIndexes};

    #[test]
    fn exact_match_returns_index() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            linear_search(&values, &5),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3)
            }
        );
    }

    #[test]
    fn exact_match_returns_index_single() {
        let values = vec![5];
        assert_eq!(
            linear_search(&values, &5),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: None
            }
        );
    }

    #[test]
    fn missing_value_upper_side() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            linear_search(&values, &6),
            SearchIndexes {
                before: Some(2),
                exact: None,
                after: Some(3)
            }
        );
    }

    #[test]
    fn missing_value_upper_side_single() {
        let values = vec![5];
        assert_eq!(
            linear_search(&values, &6),
            SearchIndexes {
                before: Some(0),
                exact: None,
                after: None
            }
        );
    }

    #[test]
    fn missing_value_lower_side() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            linear_search(&values, &4),
            SearchIndexes {
                before: Some(1),
                exact: None,
                after: Some(2)
            }
        );
    }
    #[test]
    fn missing_value_lower_side_single() {
        let values = vec![5];
        assert_eq!(
            linear_search(&values, &4),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
    }

    #[test]
    fn empty_array_returns_none() {
        let values: Vec<i32> = vec![];
        assert_eq!(linear_search(&values, &42), SearchIndexes::none());
    }

    #[test]
    fn target_below_first_value_returns_after_index_zero() {
        let values = vec![2, 4, 6, 8, 10];
        assert_eq!(
            linear_search(&values, &1),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
    }

    #[test]
    fn target_above_last_value_returns_before_last_index() {
        let values = vec![2, 4, 6, 8, 10];
        assert_eq!(
            linear_search(&values, &11),
            SearchIndexes {
                before: Some(4),
                exact: None,
                after: None
            }
        );
    }

    #[test]
    fn exact_match_at_beginning_and_end_are_returned() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            linear_search(&values, &1),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: Some(1)
            }
        );
        assert_eq!(
            linear_search(&values, &9),
            SearchIndexes {
                before: Some(3),
                exact: Some(4),
                after: None
            }
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
            linear_search(&values, &5_i32),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3)
            }
        );
        assert_eq!(
            linear_search(&values, &6_i32),
            SearchIndexes {
                before: Some(2),
                exact: None,
                after: Some(3)
            }
        );
        assert_eq!(
            linear_search(&values, &0_i32),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
        assert_eq!(
            linear_search(&values, &10_i32),
            SearchIndexes {
                before: Some(4),
                exact: None,
                after: None
            }
        );
    }
}
