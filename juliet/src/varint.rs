//! Variable length integer encoding.
//!
//! This module implements the variable length encoding of 32 bit integers, as described in the
//! juliet RFC.

use std::num::NonZeroU8;

#[derive(Copy, Clone, Debug)]
enum Varint32Result {
    Incomplete,
    TooLong,
    Overflow,
    Valid {
        // Note: `offset` is a `NonZero` type to allow niche optimization by the compiler. The
        //       expected size for this `enum` on 64 bit systems is 8 bytes.
        offset: NonZeroU8,
        value: u32,
    },
}

impl Varint32Result {
    #[track_caller]
    pub fn unwrap(self) -> (NonZeroU8, u32) {
        match self {
            Varint32Result::Incomplete | Varint32Result::TooLong | Varint32Result::Overflow => {
                panic!("`unwrap` called on invalid `Varint32Result`")
            }
            Varint32Result::Valid { offset, value } => (offset, value),
        }
    }
}

fn decode_varint32(input: &[u8]) -> Varint32Result {
    let mut value = 0u32;

    for (idx, &c) in input.iter().enumerate() {
        value |= ((c & 0b0111_1111) as u32) << (idx * 7);

        if idx > 4 && c & 0b1111_0000 != 0 {
            return Varint32Result::Overflow;
        }

        if c & 0b1000_0000 != 0 {
            if idx > 4 {
                return Varint32Result::TooLong;
            }
        } else {
            return Varint32Result::Valid {
                value,
                offset: NonZeroU8::new((idx + 1) as u8).unwrap(),
            };
        }
    }

    // We found no stop bit, so our integer is incomplete.
    Varint32Result::Incomplete
}

#[repr(transparent)]
struct Varint32([u8; 6]);

const VARINT_MASK: u8 = 0b0111_1111;

impl Varint32 {
    pub fn encode(mut value: u32) -> Self {
        let mut output = [0u8; 6];
        let mut count = 0;

        while value > 0 {
            output[count] = value as u8 & VARINT_MASK;
            value = value >> 7;
            if value > 0 {
                output[count] |= !VARINT_MASK;
                count += 1;
            }
        }

        output[5] = count as u8;
        Varint32(output)
    }
}

impl AsRef<[u8]> for Varint32 {
    fn as_ref(&self) -> &[u8] {
        let len = self.0[5] as usize + 1;
        &self.0[0..len]
    }
}

#[cfg(test)]
mod tests {
    use proptest_attr_macro::proptest;

    use crate::varint::{decode_varint32, Varint32Result};

    use super::Varint32;

    #[test]
    fn encode_known_values() {
        assert_eq!(Varint32::encode(0x00000000).as_ref(), &[0x00]);
        assert_eq!(Varint32::encode(0x00000040).as_ref(), &[0x40]);
        assert_eq!(Varint32::encode(0x0000007f).as_ref(), &[0x7f]);
        assert_eq!(Varint32::encode(0x00000080).as_ref(), &[0x80, 0x01]);
        assert_eq!(Varint32::encode(0x000000ff).as_ref(), &[0xff, 0x01]);
        assert_eq!(Varint32::encode(0x000000ff).as_ref(), &[0xff, 0x01]);
        assert_eq!(Varint32::encode(0x0000ffff).as_ref(), &[0xff, 0xff, 0x03]);
        assert_eq!(
            Varint32::encode(0xffffffff).as_ref(),
            &[0xff, 0xff, 0xff, 0xff, 0x0f]
        );

        // 0x12345678 = 0b0001   0010001   1010001   0101100   1111000
        //                0001  10010001  11010001  10101100  11111000
        //              0x  01        91        d1        ac        f8

        assert_eq!(
            Varint32::encode(0x12345678).as_ref(),
            &[0xf8, 0xac, 0xd1, 0x91, 0x01]
        );
    }

    #[track_caller]
    fn check_decode(expected: u32, input: &[u8]) {
        let decoded = decode_varint32(input);

        match decoded {
            Varint32Result::Incomplete | Varint32Result::TooLong | Varint32Result::Overflow => {
                panic!("unexpected outcome: {:?}", decoded)
            }
            Varint32Result::Valid { offset, value } => {
                assert_eq!(expected, value);
                assert_eq!(offset.get() as usize, input.len());
            }
        }
    }

    #[test]
    fn decode_known_values() {
        check_decode(0x00000000, &[0x00]);
        check_decode(0x00000040, &[0x40]);
        check_decode(0x0000007f, &[0x7f]);
        check_decode(0x00000080, &[0x80, 0x01]);
        check_decode(0x000000ff, &[0xff, 0x01]);
        check_decode(0x000000ff, &[0xff, 0x01]);
        check_decode(0x0000ffff, &[0xff, 0xff, 0x03]);
        check_decode(0xffffffff, &[0xff, 0xff, 0xff, 0xff, 0x0f]);
        check_decode(0x12345678, &[0xf8, 0xac, 0xd1, 0x91, 0x01]);
    }

    #[proptest]
    fn roundtrip_value(value: u32) {
        let encoded = Varint32::encode(value);
        let decoded = decode_varint32(encoded.as_ref());

        let (offset, decoded_value) = decoded.unwrap();
        assert_eq!(value, decoded_value);
        assert_eq!(offset.get() as usize, encoded.as_ref().len());
    }
}
