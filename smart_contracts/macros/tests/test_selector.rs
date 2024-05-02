use casper_macros::selector;
use vm_common::selector::Selector;

const A: u8 = 184;
const B: u8 = 254;
const C: u8 = 159;
const D: u8 = 127;

const FOO_SELECTOR: Selector = selector!("foo");

#[test]
fn test_selector_macro() {
    let expected = blake2_rfc::blake2b::blake2b(32, &[], b"foo");
    let first_4_bytes: [u8; 4] = (&expected.as_bytes()[0..4]).try_into().unwrap();
    assert_eq!(first_4_bytes, [A, B, C, D]);
    assert_eq!(
        FOO_SELECTOR,
        Selector::new(u32::from_be_bytes(first_4_bytes))
    );
    assert_eq!(
        format!("{:x}", FOO_SELECTOR.get()),
        format!("{:x}{:x}{:x}{:x}", A, B, C, D)
    );
}
