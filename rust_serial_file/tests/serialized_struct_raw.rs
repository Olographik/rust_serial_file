use rust_serial_file::serialized_struct::{self, DecodeRaw, EncodeRaw, SerializedStruct};

fn assert_primitive_round_trip<T>(value: T, expected: &[u8])
where
    T: DecodeRaw + EncodeRaw + SerializedStruct + PartialEq + std::fmt::Debug,
{
    let mut output = vec![0xA5; expected.len() + 1];
    let written = value.encode(&mut output);

    assert_eq!(written, expected.len());
    assert_eq!(&output[..written], expected);
    assert_eq!(output[written], 0xA5);
    assert_eq!(T::from_le_bytes_only(&output), value);
    assert_eq!(T::SIZE, std::mem::size_of::<T>());
}

#[test]
fn raw_traits_are_publicly_accessible() {
    let value = 0x1234_u16;
    let mut output = [0; 2];

    assert_eq!(value.encode(&mut output), 2);
    assert_eq!(u16::from_le_bytes_only(&output), value);
}

#[test]
fn primitive_values_encode_as_little_endian_bytes() {
    assert_primitive_round_trip(0x12_u8, &[0x12]);
    assert_primitive_round_trip(0x1234_u16, &[0x34, 0x12]);
    assert_primitive_round_trip(0x1234_5678_u32, &[0x78, 0x56, 0x34, 0x12]);
    assert_primitive_round_trip(
        0x0123_4567_89AB_CDEF_u64,
        &[0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01],
    );
    assert_primitive_round_trip(
        0x0123_4567_89AB_CDEF_0011_2233_4455_6677_u128,
        &[
            0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00, 0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45,
            0x23, 0x01,
        ],
    );
    assert_primitive_round_trip(0x1234_usize, &0x1234_usize.to_le_bytes());

    assert_primitive_round_trip(-2_i8, &(-2_i8).to_le_bytes());
    assert_primitive_round_trip(-0x1234_i16, &(-0x1234_i16).to_le_bytes());
    assert_primitive_round_trip(-0x1234_5678_i32, &(-0x1234_5678_i32).to_le_bytes());
    assert_primitive_round_trip(
        -0x0123_4567_89AB_CDEF_i64,
        &(-0x0123_4567_89AB_CDEF_i64).to_le_bytes(),
    );
    assert_primitive_round_trip(
        -0x0123_4567_89AB_CDEF_0011_2233_4455_6677_i128,
        &(-0x0123_4567_89AB_CDEF_0011_2233_4455_6677_i128).to_le_bytes(),
    );
    assert_primitive_round_trip(-0x1234_isize, &(-0x1234_isize).to_le_bytes());

    assert_primitive_round_trip(1.5_f32, &1.5_f32.to_le_bytes());
    assert_primitive_round_trip(-2.25_f64, &(-2.25_f64).to_le_bytes());
}

#[test]
fn primitive_serialized_size_matches_type_width() {
    fn assert_size<T: SerializedStruct>() {
        assert_eq!(T::SIZE, std::mem::size_of::<T>());
    }

    assert_size::<u8>();
    assert_size::<u16>();
    assert_size::<u32>();
    assert_size::<u64>();
    assert_size::<u128>();
    assert_size::<usize>();
    assert_size::<i8>();
    assert_size::<i16>();
    assert_size::<i32>();
    assert_size::<i64>();
    assert_size::<i128>();
    assert_size::<isize>();
    assert_size::<f32>();
    assert_size::<f64>();
}

#[test]
fn primitive_values_decode_and_advance_offsets() {
    let bytes = [0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0xFF];
    let mut offset = 0;

    assert_eq!(u16::from_le_bytes_slices(&bytes, &mut offset), 0x1234);
    assert_eq!(offset, 2);
    assert_eq!(u32::from_le_bytes_slices(&bytes, &mut offset), 0x1234_5678);
    assert_eq!(offset, 6);
    assert_eq!(u8::from_le_bytes_only(&bytes[6..]), 0xFF);
}

#[test]
fn checked_encode_validates_buffer_size() {
    let value = 0x1234_5678_u32;
    let mut exact = [0; 4];
    assert_eq!(value.try_encode(&mut exact).unwrap(), 4);
    assert_eq!(exact, value.to_le_bytes());

    let mut oversized = [0xA5; 6];
    assert_eq!(value.try_encode(&mut oversized).unwrap(), 4);
    assert_eq!(&oversized[..4], &value.to_le_bytes());
    assert_eq!(&oversized[4..], &[0xA5; 2]);

    let mut undersized = [0xA5; 3];
    let original = undersized;
    assert!(value.try_encode(&mut undersized).is_err());
    assert_eq!(undersized, original);
}

#[test]
fn checked_decode_validates_buffer_size() {
    let value = 0x1234_5678_u32;
    let mut oversized = value.to_le_bytes().to_vec();
    oversized.extend_from_slice(&[0xA5, 0x5A]);
    assert_eq!(u32::try_decode(&oversized).unwrap(), value);

    assert!(u32::try_decode(&value.to_le_bytes()[..3]).is_err());
}

#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq)]
struct DerivedRecord {
    id: u16,
    value: i32,
}

#[test]
fn derived_serialized_struct_uses_checked_methods() {
    let value = DerivedRecord {
        id: 0x1234,
        value: -7,
    };
    let mut bytes = [0; 6];

    assert_eq!(DerivedRecord::SIZE, 6);
    assert_eq!(value.try_encode(&mut bytes).unwrap(), bytes.len());
    assert_eq!(DerivedRecord::try_decode(&bytes).unwrap(), value);
}
