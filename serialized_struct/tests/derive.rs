//! Derive macro used through the `serialized_struct` crate itself.

use serialized_struct::{DecodeRaw, EncodeRaw, SerializedStruct};

#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq)]
struct Sample {
    tag: u8,
    count: u32,
    delta: i64,
    value: f64,
}

#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq)]
struct Pair(u16, i16);

fn sample() -> Sample {
    Sample {
        tag: 0x12,
        count: 0x3456_789A,
        delta: -2,
        value: 1.5,
    }
}

/// Bytes of `sample()` as produced before the crate was extracted from
/// `rust_serial_file`: little-endian, declaration order, no padding.
const SAMPLE_GOLDEN: [u8; 21] = [
    0x12, // tag
    0x9A, 0x78, 0x56, 0x34, // count
    0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // delta = -2
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F, // value = 1.5
];

#[test]
fn derives_through_new_crate() {
    assert_eq!(Sample::SIZE, 21);
    let mut bytes = [0; 21];
    assert_eq!(sample().try_encode(&mut bytes).unwrap(), 21);
    assert_eq!(Sample::try_decode(&bytes).unwrap(), sample());

    assert_eq!(Pair::SIZE, 4);
    let mut bytes = [0; 4];
    Pair(0x0102, -1).encode(&mut bytes);
    assert_eq!(bytes, [0x02, 0x01, 0xFF, 0xFF]);
    assert_eq!(Pair::from_le_bytes_only(&bytes), Pair(0x0102, -1));
}

#[test]
fn derived_layout_matches_golden_bytes() {
    let mut bytes = [0; 21];
    sample().encode(&mut bytes);
    assert_eq!(bytes, SAMPLE_GOLDEN);
    assert_eq!(Sample::from_le_bytes_only(&SAMPLE_GOLDEN), sample());
}
