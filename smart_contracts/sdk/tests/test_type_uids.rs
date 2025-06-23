use casper_contract_sdk::{
    common::type_uid::{TypeUid, Uid},
    macros::TypeUid,
};

#[allow(dead_code)]
#[derive(TypeUid)]
struct Struct {
    a: u64,
    b: String,
}

mod sub {
    use casper_contract_macros::TypeUid;

    #[allow(dead_code)]
    #[derive(TypeUid)]
    pub(super) struct Struct {
        field1: u64,
        field2: String,
    }
}

#[allow(dead_code)]
#[derive(TypeUid)]
struct Empty;

#[allow(dead_code)]
#[derive(TypeUid)]
struct EmptyNamedTuple(u64, String, u128);

#[allow(dead_code)]
#[derive(TypeUid)]
#[repr(u64)]
enum Foo {
    Variant1, // uid => hash_name("Variant1")
    Variant2 { a: u64, b: String },
    Variant3(u8, u16, u32, u64),
    Variant4 = 1234,
}

#[test]
fn test_struct_type_uid() {
    assert_eq!(Struct::UID, sub::Struct::UID);

    assert_eq!(
        Struct::UID,
        Uid::from_fields("Struct", &[u64::UID, String::UID])
    );

    assert_eq!(Empty::UID, Uid::from_fields("Empty", &[]));

    assert_eq!(
        EmptyNamedTuple::UID,
        Uid::from_fields("EmptyNamedTuple", &[u64::UID, String::UID, u128::UID])
    );

    assert_eq!(
        Foo::UID,
        Uid::from_fields(
            "Foo",
            &[
                Uid::from_fields("Variant1", &[]),                      // Variant1
                Uid::from_fields("Variant2", &[u64::UID, String::UID]), // Variant2
                Uid::from_fields("Variant3", &[u8::UID, u16::UID, u32::UID, u64::UID]), // Variant3
                Uid::from_fields("Variant4", &[Uid::from(1234_u64)]),
            ]
        )
    );
}
