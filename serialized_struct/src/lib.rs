//! Fixed-layout, little-endian encoding of records.
//!
//! This crate holds the traits used to turn a record into bytes and back:
//! [`EncodeRaw`], [`DecodeRaw`] and [`SerializedStruct`], with their
//! implementations for the primitive integer and float types. It is shared by
//! `rust_serial_file` (which re-exports it as
//! `rust_serial_file::serialized_struct`) and by clients that only exchange
//! records, such as a WebAssembly browser client.
//!
//! The crate depends only on `anyhow`, so it builds for
//! `wasm32-unknown-unknown`. With the `derive` feature it also re-exports the
//! `SerializedStruct` derive macro:
//!
//! ```toml
//! serialized_struct = { path = "../serialized_struct", features = ["derive"] }
//! ```
//!
//! # Byte format
//!
//! Values are written little-endian, fields in declaration order, without
//! padding. `SerializedStruct::SIZE` is the exact encoded size.
//!
//! # `usize` and `isize`
//!
//! The implementations for `usize` and `isize` used directly (for example a
//! `StructFile<usize>`) use the target's pointer width: 4 bytes on `wasm32`,
//! 8 bytes on 64-bit hosts. Data written with them is not portable between
//! such targets. Records exchanged between a browser and a host should use
//! fixed-width types (`u64`, `i64`, ...).

use std::mem::size_of;

#[cfg(feature = "derive")]
pub use serialized_struct_derive::SerializedStruct;

/// Writes a value's little-endian bytes at the start of a buffer.
pub trait EncodeRaw {
    /// Encodes `self` at the start of `data` and returns the number of bytes
    /// written.
    ///
    /// # Panics
    ///
    /// Panics if `data` is shorter than the encoded size. Use
    /// [`SerializedStruct::try_encode`] for a checked version.
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
/// Builds a value from little-endian bytes.
pub trait DecodeRaw {
    /// Decodes a value from `data` starting at `*offset`, and advances
    /// `*offset` past the bytes read.
    ///
    /// # Panics
    ///
    /// Panics if `data` is too short. Use [`SerializedStruct::try_decode`]
    /// for a checked version.
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self;

    /// Decodes a value from the start of `data`.
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

/// A record with a fixed encoded size, encodable and decodable.
pub trait SerializedStruct: DecodeRaw + EncodeRaw {
    /// Exact number of bytes written by [`EncodeRaw::encode`].
    const SIZE: usize;

    /// Encodes `self`, or returns an error when `data` is shorter than
    /// [`Self::SIZE`].
    fn try_encode(&self, data: &mut [u8]) -> anyhow::Result<usize>
    where
        Self: Sized,
    {
        if data.len() < Self::SIZE {
            anyhow::bail!(
                "cannot encode {} bytes into a {}-byte buffer",
                Self::SIZE,
                data.len()
            );
        }

        Ok(self.encode(data))
    }

    /// Decodes a value, or returns an error when `data` is shorter than
    /// [`Self::SIZE`].
    fn try_decode(data: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        if data.len() < Self::SIZE {
            anyhow::bail!(
                "cannot decode {} bytes from a {}-byte buffer",
                Self::SIZE,
                data.len()
            );
        }

        Ok(Self::from_le_bytes_only(data))
    }
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

/// Handle to a variable-length value stored outside a fixed-width record.
///
/// A record field of this type points into a companion blob file (see
/// `rust_serial_file`'s dynamic storage): `offset` is the file offset of the
/// value's bytes and `len` their number. The handle holds no I/O, so records
/// containing it still build for WebAssembly; resolving it needs the blob
/// file.
///
/// Encoded as 12 bytes: `offset` (`u64`) then `len` (`u32`), little-endian.
/// [`BlobRef::EMPTY`] (all zero) is an empty value with no storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlobRef {
    /// File offset of the first byte of the value.
    pub offset: u64,
    /// Length of the value in bytes.
    pub len: u32,
}

impl BlobRef {
    /// The empty value: no bytes and no storage.
    pub const EMPTY: Self = Self { offset: 0, len: 0 };

    /// Builds a handle from an offset and a length.
    pub const fn new(offset: u64, len: u32) -> Self {
        Self { offset, len }
    }

    /// Returns `true` when the value has no bytes. Empty values are always
    /// stored as [`BlobRef::EMPTY`].
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl EncodeRaw for BlobRef {
    fn encode(&self, data: &mut [u8]) -> usize {
        let offset = self.offset.encode(data);
        offset + self.len.encode(&mut data[offset..])
    }
}

impl DecodeRaw for BlobRef {
    fn from_le_bytes_slices(data: &[u8], offset: &mut usize) -> Self {
        let blob_offset = u64::from_le_bytes_slices(data, offset);
        let len = u32::from_le_bytes_slices(data, offset);
        Self {
            offset: blob_offset,
            len,
        }
    }
}

impl SerializedStruct for BlobRef {
    const SIZE: usize = u64::SIZE + u32::SIZE;
}
