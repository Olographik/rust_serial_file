use serialized_struct::{BlobRef, DecodeRaw, EncodeRaw, SerializedStruct};

#[test]
fn blob_ref_layout() {
    assert_eq!(BlobRef::SIZE, 12);
    let value = BlobRef::new(0x0102_0304_0506_0708, 0x0A0B_0C0D);
    let mut bytes = [0; 12];
    assert_eq!(value.encode(&mut bytes), 12);
    assert_eq!(
        bytes,
        [8, 7, 6, 5, 4, 3, 2, 1, 0x0D, 0x0C, 0x0B, 0x0A],
        "offset then len, little-endian"
    );
    assert_eq!(BlobRef::from_le_bytes_only(&bytes), value);
    assert_eq!(BlobRef::try_decode(&bytes).unwrap(), value);
    assert!(BlobRef::try_decode(&bytes[..11]).is_err());

    assert!(BlobRef::EMPTY.is_empty());
    assert_eq!(BlobRef::default(), BlobRef::EMPTY);
    assert!(!value.is_empty());
    let mut zero = [0xFF; 12];
    BlobRef::EMPTY.encode(&mut zero);
    assert_eq!(zero, [0; 12]);
}

/// A record mixing fixed fields and a `BlobRef`, as stored in a struct file.
#[derive(serialized_struct_derive::SerializedStruct, Debug, PartialEq)]
struct Named {
    time: u64,
    name: BlobRef,
}

#[test]
fn blob_ref_as_derived_field() {
    assert_eq!(Named::SIZE, 20);
    let record = Named {
        time: 9,
        name: BlobRef::new(16, 5),
    };
    let mut bytes = [0; 20];
    record.try_encode(&mut bytes).unwrap();
    assert_eq!(Named::try_decode(&bytes).unwrap(), record);
}
