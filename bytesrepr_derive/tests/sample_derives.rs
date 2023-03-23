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

#[derive(FromBytes, ToBytes, Debug, PartialEq)]
enum MyEnum {
    MyNamedVariant { a: u64, b: u32, c: String },
    MyUnnamedVariant(u64, u32, String),
    MyUnitVariant,
}

#[derive(ToBytes, FromBytes, Debug, PartialEq)]
struct MyGenericStruct<S, T>
where
    S: Debug,
{
    foo: S,
    bar: T,
}

#[derive(ToBytes, FromBytes, Debug, PartialEq)]
enum MyGenericEnum<S, T>
where
    S: Debug,
{
    Foo(S),
    Bar(T),
}

fn roundtrip_value<T>(value: T)
where
    T: FromBytes + ToBytes + PartialEq + Debug,
{
    let expected_length = value.serialized_length();
    let encoded = value.to_bytes().expect("serialization failed");
    assert_eq!(encoded.len(), expected_length);

    let (decoded, remainder) =
        <T as FromBytes>::from_bytes(&encoded).expect("deserialization failed");
    assert_eq!(remainder.len(), 0);
    assert_eq!(value, decoded);
}

mod inner {
    use super::roundtrip_value;

    // Ensure we can derive without importing the `ToBytes`/`FromBytes` traits.

    use bytesrepr_derive::{FromBytes, ToBytes};

    #[derive(Debug, ToBytes, FromBytes, PartialEq)]
    struct Foo<T> {
        a: T,
    }

    #[derive(Debug, ToBytes, FromBytes, PartialEq)]
    enum Bar<T> {
        Baz(T),
    }

    #[test]
    fn roundtrip_inner() {
        roundtrip_value(Foo { a: 123 });
        roundtrip_value(Bar::Baz(123));
    }
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

#[test]
fn roundtrip_enum_named() {
    roundtrip_value(MyEnum::MyNamedVariant {
        a: 123,
        b: 456,
        c: "hello, world".to_owned(),
    });
}

#[test]
fn roundtrip_enum_unit() {
    roundtrip_value(MyEnum::MyUnitVariant);
}

#[test]
fn roundtrip_generic_struct() {
    roundtrip_value(MyGenericStruct {
        foo: 123u32,
        bar: 56u8,
    })
}

#[test]
fn roundtrip_generic_enum() {
    roundtrip_value(MyGenericEnum::<u32, u64>::Foo(999));
    roundtrip_value(MyGenericEnum::<u32, u64>::Bar(123));
}
