use std::cmp::Ordering;

use crate::search::search_type::{SearchContext, SearchIndexes};

/// Comparator: comparator(index) -> ordering
/// Size must be at least 2
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
fn comparator_dicotomic_search_pre_filtered<F>(
    mut comparator: F,
    size: usize,
    mut lower: usize,
    mut upper: usize,
) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    // the call of this function need a size at least >= 2

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

macro_rules! size_check {
    ($size:expr, $comparator:expr) => {
        if $size == 0 {
            return SearchIndexes::none();
        }

        if $size == 1 {
            // only one element in the vector
            return match $comparator(0) {
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
    };
}

/// Comparator: comparator(index) -> ordering
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
fn comparator_dicotomic_search_size_checked<F>(mut comparator: F, size: usize) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    // at this point size is at least 2
    let lower = 0;
    let upper = size - 1;

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

    comparator_dicotomic_search_pre_filtered(comparator, size, lower, upper)
}

/// Comparator: comparator(index) -> ordering
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_dicotomic_search_no_context<F>(mut comparator: F, size: usize) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    size_check!(size, comparator);

    comparator_dicotomic_search_size_checked(comparator, size)
}

/// Comparator: comparator(index) -> ordering
///  Equivalent to array:&[T], key: &K
///     => let value = &array[index];
///     => return value.partial_cmp(key); => value> key => Greater; value == key => Equal; value < key => Lower
pub fn comparator_dicotomic_search_with_context<F>(
    mut comparator: F,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: FnMut(usize) -> Ordering,
{
    size_check!(size, comparator);

    let mut lower = 0usize;
    let mut upper = size - 1;

    if let Some(index) = context.0 {
        let current_point_comp = comparator(index);
        match current_point_comp {
            Ordering::Less => {
                // update lower key as the value is less than the key
                let start_point = index + 1;
                if start_point < size {
                    lower = start_point;
                } else {
                    // we are less and already at the end
                    // context is already on correct point
                    return SearchIndexes {
                        before: Some(index),
                        exact: None,
                        after: None,
                    };
                }
            }
            Ordering::Equal => return SearchIndexes::from_exact_point(index, size), // context is already on correct point
            Ordering::Greater => {
                // update upper key as the value is higher than the key
                if index > 0 {
                    upper = index - 1;
                } else {
                    // index is 0 and we are greater
                    // context is already on correct point
                    return SearchIndexes {
                        before: None,
                        exact: None,
                        after: Some(index),
                    };
                }
            }
        }
    } else {
        // no context, explore buffer normally
        let search = comparator_dicotomic_search_size_checked(comparator, size);
        context.update_context(&search);
        return search;
    }

    // at this point size is at least 2 and we have starting context
    // The upper and lower are valid index

    // early check of bounds, usefull ?

    // check if inside the bound first
    match comparator(lower) {
        Ordering::Less => {
            // we know here that the start point is either index + 1 or 0
            // if lower is 0, upper is index - 1
            // if lower is index + 1, upper is size - 1
            match comparator(upper) {
                Ordering::Less => {
                    // upper is either index -1 or size - 1
                    // case 1: lower is 0, upper is index - 1
                    //     => at index > key (implied from check before)
                    //     => index - 1 (upper) < key
                    //     => key is [index-1, none, index]
                    // case 2: lower is index + 1, upper is size - 1
                    //     => at index < key (implied from check before)
                    //     => size - 1 (upper) < key
                    //     => key is [size -1, none, none]
                    let search = SearchIndexes::from_before_point(upper, size);
                    context.update_context(&search);
                    return search;
                }
                Ordering::Equal => {
                    // upper is either index -1 or size - 1
                    // has we have context, the upper might not be size - 1, add check with buffer bound:
                    let search = SearchIndexes::from_exact_point(upper, size);
                    context.update_context(&search);
                    return search;
                }
                Ordering::Greater => {
                    // upper is either index -1 or size - 1
                    // key is in ] lower, upper [
                    // case 1: lower is 0, upper is index - 1
                    //     => key in ] 0, index -1 [
                    // case 2: lower is index + 1, upper is size - 1
                    //     => key in ] index +1 , size -1 [
                    // will use binary searcch
                }
            }
        }
        Ordering::Equal => {
            // lower is either 0 or index + 1
            // has we have context, the lower point might not be 0, add check with buffer bound:
            let search = SearchIndexes::from_exact_point(lower, size);
            context.update_context(&search);
            return search;
        }
        Ordering::Greater => {
            // lower is either 0 or index + 1
            // has we have context, the lower point might not be 0, add check with buffer bound:
            let search = SearchIndexes::from_after_point(lower);
            context.update_context(&search);
            return search;
        }
    }

    // bound are check, we can explore the buffer with the new bounds
    let search = comparator_dicotomic_search_pre_filtered(comparator, size, lower, upper);
    context.update_context(&search);
    return search;
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
    comparator_dicotomic_search_no_context(|i| value_getter(i).partial_cmp(target).unwrap(), size)
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

pub fn dicotomic_search_with_context_fn_mut<'a, F, T, K>(
    mut value_getter: F,
    target: &K,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: FnMut(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    comparator_dicotomic_search_with_context(
        |i| value_getter(i).partial_cmp(target).unwrap(),
        size,
        context,
    )
}

pub fn dicotomic_search_with_context_fn<'a, F, T, K>(
    value_getter: F,
    target: &K,
    size: usize,
    context: &mut SearchContext,
) -> SearchIndexes
where
    F: Fn(usize) -> &'a T,
    T: PartialOrd<K> + 'a,
{
    dicotomic_search_with_context_fn_mut(value_getter, target, size, context)
}

pub fn dicotomic_search_with_context<T, K>(
    array: &[T],
    target: &K,
    context: &mut SearchContext,
) -> SearchIndexes
where
    T: PartialOrd<K>,
{
    dicotomic_search_with_context_fn(|i| &array[i], target, array.len(), context)
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::search::{
        binary_search_v2::{SearchIndexes, dicotomic_search, dicotomic_search_with_context},
        search_type::SearchContext,
    };

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

    #[test]
    fn exact_match_returns_index_with_context() {
        let values = vec![1, 3, 5, 7, 9];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &5, &mut context),
            SearchIndexes {
                before: Some(1),
                exact: Some(2),
                after: Some(3)
            }
        );
    }

    #[test]
    fn exact_match_returns_index_single_with_context() {
        let values = vec![5];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &5, &mut context),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: None
            }
        );
    }

    #[test]
    fn missing_value_upper_side_with_context() {
        let values = vec![1, 3, 5, 7, 9];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &6, &mut context),
            SearchIndexes {
                before: Some(2),
                exact: None,
                after: Some(3)
            }
        );
    }

    #[test]
    fn missing_value_upper_side_single_with_context() {
        let values = vec![5];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &6, &mut context),
            SearchIndexes {
                before: Some(0),
                exact: None,
                after: None
            }
        );
    }

    #[test]
    fn missing_value_lower_side_with_context() {
        let values = vec![1, 3, 5, 7, 9];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &4, &mut context),
            SearchIndexes {
                before: Some(1),
                exact: None,
                after: Some(2)
            }
        );
    }
    #[test]
    fn missing_value_lower_side_single_with_context() {
        let values = vec![5];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &4, &mut context),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
    }

    #[test]
    fn empty_array_returns_none_with_context() {
        let values: Vec<i32> = vec![];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &42, &mut context),
            SearchIndexes::none()
        );
    }

    #[test]
    fn target_below_first_value_returns_after_index_zero_with_context() {
        let values = vec![2, 4, 6, 8, 10];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &1, &mut context),
            SearchIndexes {
                before: None,
                exact: None,
                after: Some(0)
            }
        );
    }

    #[test]
    fn target_above_last_value_returns_before_last_index_with_context() {
        let values = vec![2, 4, 6, 8, 10];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &11, &mut context),
            SearchIndexes {
                before: Some(4),
                exact: None,
                after: None
            }
        );
    }

    #[test]
    fn exact_match_at_beginning_and_end_are_returned_with_context() {
        let values = vec![1, 3, 5, 7, 9];
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &1, &mut context),
            SearchIndexes {
                before: None,
                exact: Some(0),
                after: Some(1)
            }
        );
        let mut context = SearchContext::of(1);
        assert_eq!(
            dicotomic_search_with_context(&values, &9, &mut context),
            SearchIndexes {
                before: Some(3),
                exact: Some(4),
                after: None
            }
        );
    }
}
