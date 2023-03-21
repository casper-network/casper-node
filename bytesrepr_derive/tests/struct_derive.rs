use bytesrepr_derive::ToBytes;
use casper_types::bytesrepr::ToBytes;

#[derive(ToBytes, Debug)]
struct MyNamedStruct {
    a: u64,
    b: u32,
    c: String,
}

#[derive(ToBytes, Debug)]
struct MyUnnamedStruct(u64, u32, String);

fn roundtrip_value<T>(value: T)
where
    T: ToBytes,
{
    let encoded = value.to_bytes();
    // TODO: Deserialize.
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
