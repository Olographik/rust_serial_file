pub mod binary_search;
pub mod binary_search_v2;
// Uses `Tester::execute`, which draws from OS entropy through `rand`.
#[cfg(all(feature = "performance", feature = "rand"))]
pub mod compare_binary_search;
pub mod linear_search;
pub mod search_type;
