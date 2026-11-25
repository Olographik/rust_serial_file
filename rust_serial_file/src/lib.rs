pub mod adapter;
pub mod file;
/// Record encoding traits, re-exported from the `serialized_struct` crate so
/// the historical path `rust_serial_file::serialized_struct` keeps working.
pub use serialized_struct;
