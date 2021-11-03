//! Checksummed hex encoding following an [EIP-55][1]-like scheme.
//!
//! [1]: https://eips.ethereum.org/EIPS/eip-55

use alloc::{string::String, vec::Vec};
use core::ops::RangeInclusive;

use base16;
use blake2::{Blake2b, Digest};

/// The number of input bytes, at or below which [`encode`] will checksum-encode the output.
pub const SMALL_BYTES_COUNT: usize = 75;

const HEX_CHARS: [char; 22] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'A', 'B', 'C',
    'D', 'E', 'F',
];

/// Takes a slice of bytes and breaks it up into a vector of *nibbles* (ie, 4-bit values)
/// represented as `u8`s.
fn bytes_to_nibbles<'a, T: 'a + AsRef<[u8]>>(input: &'a T) -> impl Iterator<Item = u8> + 'a {
    input
        .as_ref()
        .iter()
        .flat_map(move |byte| [4, 0].iter().map(move |offset| (byte >> offset) & 0x0f))
}

/// Takes a slice of bytes and outputs an infinite cyclic stream of bits for those bytes.
fn bytes_to_bits_cycle(bytes: Vec<u8>) -> impl Iterator<Item = bool> {
    bytes
        .into_iter()
        .cycle()
        .flat_map(move |byte| (0..8usize).map(move |offset| ((byte >> offset) & 0x01) == 0x01))
}

/// Computes a Blake2b hash.
fn blake2b_hash(data: impl AsRef<[u8]>) -> Vec<u8> {
    let mut hasher = Blake2b::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// If `input` is not greater than [`SMALL_BYTES_COUNT`], returns the bytes encoded as hexadecimal
/// with mixed-case based checksums following a scheme similar to [EIP-55][1].  If `input` is
/// greater than `SMALL_BYTES_COUNT`, no mixed-case checksumming is applied, and lowercase hex is
/// returned.
///
/// Key differences:
///   - Works on any length of data up to `SMALL_BYTES_COUNT`, not just 20-byte addresses
///   - Uses Blake2b hashes rather than Keccak
///   - Uses hash bits rather than nibbles
///
/// [1]: https://eips.ethereum.org/EIPS/eip-55
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    if input.as_ref().len() > SMALL_BYTES_COUNT {
        return base16::encode_lower(&input);
    }
    encode_iter(&input).collect()
}

/// `encode` but it returns an iterator.
fn encode_iter<'a, T: 'a + AsRef<[u8]>>(input: &'a T) -> impl Iterator<Item = char> + 'a {
    let nibbles = bytes_to_nibbles(input);
    let mut hash_bits = bytes_to_bits_cycle(blake2b_hash(input.as_ref()));
    nibbles.map(move |mut nibble| {
        // Base 16 numbers greater than 10 are represented by the ascii characters a through f.
        if nibble >= 10 && hash_bits.next().unwrap_or(true) {
            // We are using nibble to index HEX_CHARS, so adding 6 to nibble gives us the index
            // of the uppercase character. HEX_CHARS[10] == 'a', HEX_CHARS[16] == 'A'.
            nibble += 6;
        }
        HEX_CHARS[nibble as usize]
    })
}

/// Returns true if all chars in a string are uppercase or lowercase.
/// Returns false if the string is mixed case or if there are no alphabetic chars.
fn string_is_same_case<T: AsRef<[u8]>>(s: T) -> bool {
    const LOWER_RANGE: RangeInclusive<u8> = b'a'..=b'f';
    const UPPER_RANGE: RangeInclusive<u8> = b'A'..=b'F';

    let mut chars = s
        .as_ref()
        .iter()
        .filter(|c| LOWER_RANGE.contains(c) || UPPER_RANGE.contains(c));

    match chars.next() {
        Some(first) => {
            let is_upper = UPPER_RANGE.contains(first);
            chars.all(|c| UPPER_RANGE.contains(c) == is_upper)
        }
        None => {
            // String has no actual characters.
            true
        }
    }
}

/// Decodes a mixed-case hexadecimal string, verifying that it conforms to the checksum scheme
/// similar to scheme in [EIP-55][1].
///
/// Key differences:
///   - Works on any length of (decoded) data up to `SMALL_BYTES_COUNT`, not just 20-byte addresses
///   - Uses Blake2b hashes rather than Keccak
///   - Uses hash bits rather than nibbles
///
/// For backward compatibility: if the hex string is all uppercase or all lowercase, the check is
/// skipped.
///
/// [1]: https://eips.ethereum.org/EIPS/eip-55
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base16::DecodeError> {
    let bytes = base16::decode(input.as_ref())?;

    // If the string was not small or not mixed case, don't verify the checksum.
    if bytes.len() > SMALL_BYTES_COUNT || string_is_same_case(input.as_ref()) {
        return Ok(bytes);
    }

    encode_iter(&bytes)
        .zip(input.as_ref().iter())
        .enumerate()
        .try_for_each(|(index, (expected_case_hex_char, &input_hex_char))| {
            if expected_case_hex_char as u8 == input_hex_char {
                Ok(())
            } else {
                Err(base16::DecodeError::InvalidByte {
                    index,
                    byte: expected_case_hex_char as u8,
                })
            }
        })?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use proptest::{
        collection::vec,
        prelude::{any, prop_assert, prop_assert_eq},
    };
    use proptest_attr_macro::proptest;

    use super::*;

    #[test]
    fn should_encode_empty_input() {
        let input = [];
        let actual = encode(&input);
        assert!(actual.is_empty());
    }

    #[test]
    fn string_is_same_case_true_when_same_case() {
        let input = "aaaaaaaaaaa";
        assert!(string_is_same_case(input));

        let input = "AAAAAAAAAAA";
        assert!(string_is_same_case(input));
    }

    #[test]
    fn string_is_same_case_false_when_mixed_case() {
        let input = "aAaAaAaAaAa";
        assert!(!string_is_same_case(input));
    }

    #[test]
    fn string_is_same_case_no_alphabetic_chars_in_string() {
        let input = "424242424242";
        assert!(string_is_same_case(input));
    }

    #[test]
    fn should_checksum_encode_only_if_small() {
        let input = [255; SMALL_BYTES_COUNT + 1];
        let small_output = encode(&input[..SMALL_BYTES_COUNT]);
        assert!(!string_is_same_case(&small_output));

        let large_output = encode(&input);
        assert!(string_is_same_case(&large_output));
    }

    #[proptest]
    fn hex_roundtrip(input: Vec<u8>) {
        prop_assert_eq!(
            input.clone(),
            decode(&encode(&input)).expect("Failed to decode input.")
        );
    }

    proptest::proptest! {
        #[test]
        fn should_fail_on_invalid_checksum(input in vec(any::<u8>(), 0..75)) {
            let encoded = encode(&input);

            // Swap the case of the first letter in the checksum hex-encoded value.
            let mut expected_error = None;
            let mutated: String = encoded
                .char_indices()
                .map(|(index, mut c)| {
                    if expected_error.is_some() || c.is_ascii_digit() {
                        return c;
                    }
                    expected_error = Some(base16::DecodeError::InvalidByte {
                        index,
                        byte: c as u8,
                    });
                    if c.is_ascii_uppercase() {
                        c.make_ascii_lowercase();
                    } else {
                        c.make_ascii_uppercase();
                    }
                    c
                })
                .collect();

            // If the encoded form is now all the same case or digits, just return.
            if string_is_same_case(&mutated) {
                return Ok(());
            }

            // Assert we can still decode to original input using `base16::decode`.
            prop_assert_eq!(
                input,
                base16::decode(&mutated).expect("Failed to decode input.")
            );

            // Assert decoding using `checksummed_hex::decode` returns the expected error.
            prop_assert_eq!(expected_error.unwrap(), decode(&mutated).unwrap_err())
        }
    }

    #[proptest]
    fn hex_roundtrip_sanity(input: Vec<u8>) {
        prop_assert!(matches!(decode(&encode(&input)), Ok(_)))
    }

    #[proptest]
    fn is_same_case_uppercase(input: String) {
        let input = input.to_uppercase();
        prop_assert!(string_is_same_case(&input));
    }

    #[proptest]
    fn is_same_case_lowercase(input: String) {
        let input = input.to_lowercase();
        prop_assert!(string_is_same_case(&input));
    }

    #[proptest]
    fn is_not_same_case(input: String) {
        let input = format!("aA{}", input);
        prop_assert!(!string_is_same_case(&input));
    }
}
