use std::{cmp::Ordering, fmt::Debug};

use crate::search::search_type::SearchIndexes;

/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
fn comparator_dicotomic_search_pre_filtered<F>(mut comparator: F, size: usize) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    // the call of this function need a size at least >= 2
    let mut lower = 0usize;
    let mut upper = size - 1;

    // mean none in this case
    let mut cmp = Ordering::Equal;

    while lower < upper {
        let mid = lower + (upper - lower) / 2;
        cmp = comparator(mid);
        match cmp {
            Ordering::Less => lower = mid + 1,
            Ordering::Equal => {
                return SearchIndexes {
                    before: if mid > 0 { Some(mid - 1) } else { None },
                    exact: Some(mid),
                    after: if mid < size { Some(mid + 1) } else { None },
                };
            }
            _ => upper = mid,
        }
    }

    match cmp {
        Ordering::Less => SearchIndexes {
            // lower was mid + 1
            before: Some(lower - 1),
            exact: None,
            after: Some(lower),
        },
        Ordering::Equal => SearchIndexes::none(),
        Ordering::Greater => SearchIndexes {
            before: Some(lower - 1),
            exact: None,
            after: Some(lower),
        },
    }
}

/// Comparator: comparator(index) -> ordering
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_dicotomic_search<F>(mut comparator: F, size: usize) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    if size == 0 {
        return SearchIndexes::none();
    }
    let lower = 0usize;
    let upper = size - 1;

    if size == 1 {
        // only one element in the vector
        return match comparator(lower) {
            Ordering::Less => SearchIndexes {
                before: Some(lower),
                exact: None,
                after: None,
            },
            Ordering::Equal => SearchIndexes {
                before: None,
                exact: Some(lower),
                after: None,
            },
            Ordering::Greater => SearchIndexes {
                before: None,
                exact: None,
                after: Some(lower),
            },
        };
    }

    // at this point size is at least 2

    // check if inside the array first
    match comparator(lower) {
        Ordering::Less => {
            match comparator(upper) {
                Ordering::Less => {
                    return SearchIndexes {
                        before: Some(upper),
                        exact: None,
                        after: None,
                    };
                }
                Ordering::Equal => {
                    return SearchIndexes {
                        before: Some(upper - 1),
                        exact: Some(upper),
                        after: None,
                    };
                }
                Ordering::Greater => {
                    // will use binary searcch
                }
            }
        }
        Ordering::Equal => {
            return SearchIndexes {
                before: None,
                exact: Some(lower),
                // lower is 0 and size is at least 2
                after: Some(lower + 1),
            };
        }
        Ordering::Greater => {
            return SearchIndexes {
                before: None,
                exact: None,
                after: Some(lower),
            };
        }
    }

    comparator_dicotomic_search_pre_filtered(comparator, size)
}

pub fn dicotomic_search_fn_mut<'a, F, T, K>(
    mut value_getter: F,
    target: &K,
    size: usize,
) -> SearchIndexes
where
    F: FnMut(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    comparator_dicotomic_search(|i| value_getter(i).partial_cmp(target).unwrap(), size)
}

pub fn dicotomic_search_fn<'a, F, T, K>(value_getter: F, target: &K, size: usize) -> SearchIndexes
where
    F: Fn(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    dicotomic_search_fn_mut(value_getter, target, size)
}

pub fn dicotomic_search<T, K>(array: &[T], target: &K) -> SearchIndexes
where
    T: PartialOrd<K>,
{
    dicotomic_search_fn(|i| &array[i], target, array.len())
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::search::binary_search_v2::{SearchIndexes, dicotomic_search};

    #[test]
    fn exact_match_returns_index() {
        let values = vec![1, 3, 5, 7, 9];
        assert_eq!(
            dicotomic_search(&values, &5),
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
            dicotomic_search(&values, &5),
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
            dicotomic_search(&values, &6),
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
            dicotomic_search(&values, &6),
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
            dicotomic_search(&values, &4),
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
            dicotomic_search(&values, &4),
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
        assert_eq!(dicotomic_search(&values, &42), SearchIndexes::none());
    }

    #[test]
    fn target_below_first_value_returns_after_index_zero() {
        let values = vec![2, 4, 6, 8, 10];
        assert_eq!(
            dicotomic_search(&values, &1),
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
            dicotomic_search(&values, &11),
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
            dicotomic_search(&values, &1),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: Some(1)
            }
        );
        assert_eq!(
            dicotomic_search(&values, &9),
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
            dicotomic_search(&values, &5_i32),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3)
            }
        );
        assert_eq!(
            dicotomic_search(&values, &6_i32),
            SearchIndexes {
                before: Some(2),
                exact: None,
                after: Some(3)
            }
        );
        assert_eq!(
            dicotomic_search(&values, &0_i32),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
        assert_eq!(
            dicotomic_search(&values, &10_i32),
            SearchIndexes {
                before: Some(4),
                exact: None,
                after: None
            }
        );
    }
}
