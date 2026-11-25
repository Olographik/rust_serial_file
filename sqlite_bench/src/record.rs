//! The benchmark record, its deterministic generator and a checksum used to
//! check that both storages hold the same data.

use std::cmp::Ordering;

use rust_serial_file::serialized_struct;
use serialized_struct_derive::SerializedStruct;
use utils::random::{IndexRng, SplitMix64};

/// One time-series sample: 24 bytes, ordered by `time`.
///
/// Stored in SQLite as
/// `sample(time INTEGER PRIMARY KEY, sensor INTEGER NOT NULL, flags INTEGER NOT NULL, value REAL NOT NULL) WITHOUT ROWID`.
#[derive(SerializedStruct, Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Unique, strictly increasing key.
    pub time: u64,
    /// Sensor identifier, `0..64`.
    pub sensor: u32,
    /// Arbitrary flags.
    pub flags: u32,
    /// Measured value, `0.0..100.0`.
    pub value: f64,
}

/// Records are ordered by `time` only, so a `time` can be searched for.
impl PartialEq<u64> for Sample {
    fn eq(&self, time: &u64) -> bool {
        self.time == *time
    }
}

impl PartialOrd<u64> for Sample {
    fn partial_cmp(&self, time: &u64) -> Option<Ordering> {
        Some(self.time.cmp(time))
    }
}

/// Distance between consecutive time slots. A sample's time is its slot
/// plus a jitter in `0..TIME_JITTER`, so `time + 1` is never another
/// sample's time (see [`missing_time_after`]).
pub const TIME_STEP: u64 = 16;
const TIME_JITTER: u64 = 8;

/// Generates `count` samples from `seed`: the same seed always gives the
/// same samples, with strictly increasing times.
pub fn generate(count: usize, seed: u64) -> Vec<Sample> {
    let mut rng = SplitMix64::new(seed);
    (0..count as u64)
        .map(|slot| {
            let jitter = rng.next_u64() % TIME_JITTER;
            Sample {
                time: slot * TIME_STEP + jitter,
                sensor: rng.next_index(64) as u32,
                flags: rng.next_u64() as u32,
                // 53 random bits mapped to 0.0..100.0.
                value: (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64 * 100.0,
            }
        })
        .collect()
}

/// A time that no generated sample has: just after `sample.time`, before
/// the next slot.
pub fn missing_time_after(sample: &Sample) -> u64 {
    sample.time + 1
}

/// Order-sensitive checksum (FNV-1a over every field's little-endian bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checksum {
    hash: u64,
    count: u64,
}

impl Default for Checksum {
    fn default() -> Self {
        Self::new()
    }
}

impl Checksum {
    /// An empty checksum.
    pub const fn new() -> Self {
        Self {
            hash: 0xcbf2_9ce4_8422_2325,
            count: 0,
        }
    }

    /// Adds raw bytes.
    pub fn add_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Adds one sample.
    pub fn add(&mut self, sample: &Sample) {
        self.add_bytes(&sample.time.to_le_bytes());
        self.add_bytes(&sample.sensor.to_le_bytes());
        self.add_bytes(&sample.flags.to_le_bytes());
        self.add_bytes(&sample.value.to_bits().to_le_bytes());
        self.count += 1;
    }

    /// Adds one record made of a time and a text value.
    pub fn add_named(&mut self, time: u64, name: &str) {
        self.add_bytes(&time.to_le_bytes());
        self.add_bytes(&(name.len() as u64).to_le_bytes());
        self.add_bytes(name.as_bytes());
        self.count += 1;
    }

    /// Number of records added.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The hash value.
    pub fn value(&self) -> u64 {
        self.hash
    }
}

/// Checksum of a slice of samples.
pub fn checksum_of(samples: &[Sample]) -> Checksum {
    let mut checksum = Checksum::new();
    samples.iter().for_each(|sample| checksum.add(sample));
    checksum
}

/// Generates `count` names of 16 to 256 bytes from `seed`, for the dynamic
/// string benchmark.
pub fn generate_names(count: usize, seed: u64) -> Vec<String> {
    let mut rng = SplitMix64::new(seed);
    (0..count)
        .map(|index| {
            let len = 16 + rng.next_index(241);
            let mut name = format!("sensor-{index}-");
            while name.len() < len {
                name.push(char::from(b'a' + rng.next_index(26) as u8));
            }
            name.truncate(len);
            name
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rust_serial_file::serialized_struct::SerializedStruct;

    use super::{Sample, TIME_STEP, checksum_of, generate, generate_names, missing_time_after};

    #[test]
    fn generator_is_deterministic() {
        assert_eq!(Sample::SIZE, 24);
        let first = generate(1_000, 7);
        assert_eq!(first, generate(1_000, 7));
        assert_ne!(first, generate(1_000, 8));
        for pair in first.windows(2) {
            assert!(pair[0].time < pair[1].time);
            assert!(missing_time_after(&pair[0]) < pair[1].time);
        }
        for (slot, sample) in first.iter().enumerate() {
            assert_eq!(sample.time / TIME_STEP, slot as u64);
            assert!(sample.sensor < 64);
            assert!((0.0..100.0).contains(&sample.value));
            assert!(sample < &missing_time_after(sample));
            assert!(sample == &sample.time);
        }
        assert_eq!(checksum_of(&first), checksum_of(&generate(1_000, 7)));
        assert_ne!(checksum_of(&first), checksum_of(&first[1..]));

        let names = generate_names(500, 3);
        assert_eq!(names, generate_names(500, 3));
        assert!(names.iter().all(|name| (16..=256).contains(&name.len())));
    }
}
