use serialized_struct::SerializedStruct;

#[derive(SerializedStruct)]
struct Row {
    time: u64,
    index: usize,
}

fn main() {}
