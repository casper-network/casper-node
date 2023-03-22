use std::fmt::Debug;

use bytesrepr_derive::{FromBytes, ToBytes};
use casper_types::bytesrepr::{FromBytes, ToBytes};

#[derive(FromBytes, ToBytes, Debug, PartialEq)]
struct MyNamedStruct {
    a: u64,
    b: u32,
    c: String,
}

#[derive(FromBytes, ToBytes, Debug, PartialEq)]
struct MyUnnamedStruct(u64, u32, String);

#[derive(ToBytes, Debug, PartialEq)]
// #[derive(FromBytes, ToBytes, Debug, PartialEq)]
enum MyEnum {
    MyNamedVariant { a: u64, b: u32, c: String },
    MyUnnamedVariant(u64, u32, String),
}

fn roundtrip_value<T>(value: T)
where
    T: FromBytes + ToBytes + PartialEq + Debug,
{
    let encoded = value.to_bytes().expect("serialization failed");
    let (decoded, remainder) =
        <T as FromBytes>::from_bytes(&encoded).expect("deserialization failed");
    assert_eq!(remainder.len(), 0);
    assert_eq!(value, decoded);
}

#[test]
fn roundtrip_named() {
    roundtrip_value(MyNamedStruct {
        a: 123,
        b: 456,
        c: "hello, world".to_owned(),
    });
}

#[test]
fn roundtrip_unnamed() {
    roundtrip_value(MyUnnamedStruct(123, 456, "hello, world".to_owned()));
}

// #[test]
// fn roundtrip_enum_named() {
//     roundtrip_value(MyEnum::MyNamedVariant {
//         a: 123,
//         b: 456,
//         c: "hello, world".to_owned(),
//     });
// }
