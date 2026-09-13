use std::mem::size_of;

pub trait EncodeRaw {
    fn encode(&self, data: &mut [u8]) -> usize;
}

macro_rules! impl_encode_raw {
    ($($t:ty), *) => {
        $(impl EncodeRaw for $t {
            fn encode(&self, data: &mut [u8]) -> usize {
                let offset = size_of::<$t>();
                data[0..offset].copy_from_slice(&self.to_le_bytes());
                offset
            }
        })*
    };
}
pub trait DecodeRaw {
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self;

    fn from_le_bytes_only(data: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut offset = 0;
        Self::from_le_bytes_slices(data, &mut offset)
    }
}

macro_rules! impl_decode_raw {
    ($($t:ty), *) => {
        $(impl DecodeRaw for $t {
            fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self {
                let start = *offset;
                *offset += size_of::<$t>();
                Self::from_le_bytes(data[start..*offset].try_into().unwrap())
            }
        })*
    };
}

pub trait SerializedStruct: DecodeRaw + EncodeRaw {
    const SIZE: usize;
}

macro_rules! impl_raw_serialized_struct {
    ($($t:ty), *) => {
        $(impl_encode_raw!($t);
        impl_decode_raw!($t);
        impl SerializedStruct for $t {
            const SIZE: usize = size_of::<$t>();
        })*
    };
}

impl_raw_serialized_struct!(u8, u16, u32, u64, u128, usize);
impl_raw_serialized_struct!(i8, i16, i32, i64, i128, isize);

// impl_raw_serialized_struct!(f16);
impl_raw_serialized_struct!(f32, f64);
// impl_raw_serialized_struct!(f128);
