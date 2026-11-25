use serialized_struct_derive::{DecodeRaw, EncodeRaw};

#[derive(EncodeRaw)]
struct Encoded {
    len: usize,
}

#[derive(DecodeRaw)]
struct Decoded {
    offset: isize,
}

fn main() {}
