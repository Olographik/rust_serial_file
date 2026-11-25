//! Injectable random index generators.
//!
//! [`IndexRng`] is the only thing [`Tester`](crate::performance::Tester)
//! needs from a random generator: a value in `0..upper`. Callers choose the
//! generator:
//!
//! - any `rand` generator, with the `rand` feature (blanket implementation);
//! - [`SplitMix64`], a small deterministic generator with no dependency;
//! - [`FnIndexRng`], wrapping a closure, for example one that calls a
//!   JavaScript random source in a WebAssembly build.
//!
//! None of these generators is cryptographically secure. They are meant for
//! ordering benchmark actions.

/// A source of random indices.
pub trait IndexRng {
    /// Returns a value in `0..upper`.
    ///
    /// `Tester` never passes `upper == 0`. Implementations may panic on it.
    /// A value outside the range is a bug of the implementation; `Tester`
    /// reduces it with `% upper` rather than indexing out of bounds.
    fn next_index(&mut self, upper: usize) -> usize;
}

/// Every `rand` generator is an [`IndexRng`].
#[cfg(feature = "rand")]
impl<R: rand::Rng + ?Sized> IndexRng for R {
    fn next_index(&mut self, upper: usize) -> usize {
        use rand::RngExt;
        self.random_range(0..upper)
    }
}

/// Deterministic 64-bit generator (SplitMix64, by Sebastiano Vigna).
///
/// The same seed gives the same sequence on every platform: the state and
/// arithmetic are 64-bit, independent of `usize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from a seed.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns the next 64-bit value of the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl IndexRng for SplitMix64 {
    /// Maps the next 64-bit value to `0..upper` with a multiply-shift
    /// (Lemire), whose bias is at most `upper / 2^64`.
    ///
    /// # Panics
    ///
    /// Panics if `upper` is 0.
    fn next_index(&mut self, upper: usize) -> usize {
        assert!(upper > 0, "SplitMix64::next_index called with upper == 0");
        let upper = upper as u64; // lossless: usize is at most 64 bits wide
        let index = (u128::from(self.next_u64()) * u128::from(upper)) >> 64;
        // index < upper <= usize::MAX, so the conversion cannot fail.
        usize::try_from(index).expect("index is below upper")
    }
}

/// Adapts a closure `FnMut(upper) -> index` into an [`IndexRng`].
///
/// ```
/// use utils::random::{FnIndexRng, IndexRng};
///
/// let mut always_first = FnIndexRng::new(|_upper| 0);
/// assert_eq!(always_first.next_index(5), 0);
/// ```
pub struct FnIndexRng<F>(F);

impl<F: FnMut(usize) -> usize> FnIndexRng<F> {
    /// Wraps `f`, which receives `upper` and returns an index.
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F: FnMut(usize) -> usize> IndexRng for FnIndexRng<F> {
    fn next_index(&mut self, upper: usize) -> usize {
        (self.0)(upper)
    }
}

#[cfg(test)]
mod tests {
    use super::{FnIndexRng, IndexRng, SplitMix64};

    #[test]
    fn splitmix_reference_values() {
        // Reference outputs of the published SplitMix64 for seed 1234567.
        let mut rng = SplitMix64::new(1_234_567);
        let expected = [
            6_457_827_717_110_365_317,
            3_203_168_211_198_807_973,
            9_817_491_932_198_370_423,
            4_593_380_528_125_082_431,
            16_408_922_859_458_223_821,
        ];
        for value in expected {
            assert_eq!(rng.next_u64(), value);
        }
    }

    #[test]
    fn splitmix_index_in_range() {
        let mut rng = SplitMix64::new(42);
        let mut seen = [0_usize; 7];
        for _ in 0..100_000 {
            let index = rng.next_index(7);
            assert!(index < 7);
            seen[index] += 1;
        }
        // Every value appears, roughly uniformly (expected 14 286 each).
        for count in seen {
            assert!((13_000..15_500).contains(&count), "{seen:?}");
        }
        assert_eq!(rng.next_index(1), 0);
    }

    #[test]
    #[should_panic(expected = "upper == 0")]
    fn splitmix_rejects_empty_range() {
        SplitMix64::new(0).next_index(0);
    }

    #[test]
    fn fn_index_rng_returns_closure_values() {
        let mut values = [3, 1, 4].into_iter();
        let mut rng = FnIndexRng::new(move |upper| {
            assert_eq!(upper, 5);
            values.next().unwrap()
        });
        let dynamic: &mut dyn IndexRng = &mut rng;
        assert_eq!(dynamic.next_index(5), 3);
        assert_eq!(dynamic.next_index(5), 1);
        assert_eq!(rng.next_index(5), 4);
    }

    #[cfg(feature = "rand")]
    #[test]
    fn rand_generator_is_index_rng() {
        use rand::{SeedableRng, rngs::StdRng};

        fn draw<R: IndexRng + ?Sized>(rng: &mut R) -> usize {
            rng.next_index(10)
        }

        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..1_000 {
            assert!(draw(&mut rng) < 10);
        }
    }
}
