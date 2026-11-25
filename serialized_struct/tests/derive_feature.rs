//! The `derive` feature re-exports the macro next to the trait, so one import
//! gives both.
#![cfg(feature = "derive")]

use serialized_struct::SerializedStruct;

#[derive(SerializedStruct, Debug, PartialEq)]
struct Message {
    id: u64,
    value: f32,
}

#[test]
fn derives_with_reexported_macro() {
    assert_eq!(Message::SIZE, 12);
    let message = Message { id: 7, value: 2.0 };
    let mut bytes = [0; 12];
    message.try_encode(&mut bytes).unwrap();
    assert_eq!(Message::try_decode(&bytes).unwrap(), message);
}
