#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(feature = "std")]
use std::{collections::BTreeMap, vec::Vec};

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use core::{array::TryFromSliceError, convert::TryFrom};

#[cfg(feature = "std")]
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
#[cfg(feature = "std")]
use hex_buffer_serde::{Hex, HexForm};

#[cfg(feature = "std")]
use itertools::Itertools;

use bytesrepr::{FromBytes, ToBytes};

/// The hash digest; a wrapped `u8` array.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Default)]
#[cfg_attr(
    feature = "std",
    derive(
        schemars::JsonSchema,
        serde::Serialize,
        serde::Deserialize,
        datasize::DataSize
    ),
    schemars(with = "String", description = "Hex-encoded hash digest."),
    serde(deny_unknown_fields)
)]
pub struct Digest(
    #[cfg_attr(
        feature = "std",
        schemars(skip, with = "String"),
        serde(with = "HexForm::<[u8; Digest::LENGTH]>")
    )]
    [u8; Digest::LENGTH],
);

impl Digest {
    /// The number of bytes in a digest hash
    pub const LENGTH: usize = 32;

    /// Creates a 32-byte hash digest from a given a piece of data
    #[cfg(feature = "std")]
    pub fn hash<T: AsRef<[u8]>>(data: T) -> Digest {
        let mut result = [0; Digest::LENGTH];

        let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");
        hasher.update(data);
        hasher.finalize_variable(|slice| {
            result.copy_from_slice(slice);
        });
        Digest(result)
    }

    /// Hashes a pair of byte slices into a single [`Digest`]
    #[cfg(feature = "std")]
    pub fn hash_pair<T: AsRef<[u8]>, U: AsRef<[u8]>>(data1: T, data2: U) -> Digest {
        let mut result = [0; Digest::LENGTH];
        let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
        hasher.update(data1);
        hasher.update(data2);
        hasher.finalize_variable(|slice| {
            result.copy_from_slice(slice);
        });
        Digest(result)
    }

    /// Returns a reference to the underlying value.
    pub fn inner(&self) -> &[u8; Digest::LENGTH] {
        &self.0
    }

    /// Returns the underlying value.
    pub fn value(self) -> [u8; Digest::LENGTH] {
        self.0
    }

    /// Returns a `Digest` parsed from a hex-encoded `Digest`.
    #[cfg(feature = "std")]
    pub fn from_hex<T: AsRef<[u8]>>(hex_input: T) -> Result<Self, hex::FromHexError> {
        let mut inner = [0; Digest::LENGTH];
        hex::decode_to_slice(hex_input, &mut inner)?;
        Ok(Digest(inner))
    }
}

impl ToBytes for Digest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Digest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        FromBytes::from_bytes(bytes)
            .map(|(inner, remainder): ([u8; Digest::LENGTH], _)| (Digest(inner), remainder))
    }
}

#[cfg(feature = "std")]
impl std::fmt::LowerHex for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let hex_string = base16::encode_lower(self.inner());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::UpperHex for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let hex_string = base16::encode_upper(self.inner());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:10}", base16::encode_lower(&self.0))
    }
}

#[cfg(feature = "std")]
impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl From<[u8; Digest::LENGTH]> for Digest {
    fn from(arr: [u8; Digest::LENGTH]) -> Self {
        Digest(arr)
    }
}

impl<'a> TryFrom<&'a [u8]> for Digest {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Digest, Self::Error> {
        <[u8; Digest::LENGTH]>::try_from(slice).map(Digest)
    }
}

impl AsRef<[u8]> for Digest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<Digest> for [u8; Digest::LENGTH] {
    fn from(hash: Digest) -> Self {
        hash.0
    }
}

#[cfg(feature = "std")]
impl hex::FromHex for Digest {
    type Error = hex::FromHexError;

    fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        Ok(Digest(hex::FromHex::from_hex(hex)?))
    }
}

/// Sentinel hash to be used for hashing options in the case of [None].
pub const SENTINEL0: Digest = Digest([0u8; Digest::LENGTH]);
/// Sentinel hash to be used by [hash_slice_rfold]. Terminates the fold.
pub const SENTINEL1: Digest = Digest([1u8; Digest::LENGTH]);
/// Sentinel hash to be used by [hash_vec_merkle_tree] in the case of an empty list.
pub const SENTINEL2: Digest = Digest([2u8; Digest::LENGTH]);

/// Hashes a [`Vec`] of [`Digest`]s into a single [`Digest`] by constructing a [Merkle tree][1].
/// Reduces pairs of elements in the [`Vec`] by repeatedly calling [`Digest::hash_pair`].
/// This hash procedure is suited to hashing `BTree`s.
///
/// The pattern of hashing is as follows.  It is akin to [graph reduction][2]:
///
/// ```text
/// a b c d e f
/// |/  |/  |/
/// g   h   i
/// | /   /
/// |/   /
/// j   k
/// | /
/// |/
/// l
/// ```
///
/// Returns the [`SENTINEL2`] when the input is empty.
///
/// [1]: https://en.wikipedia.org/wiki/Merkle_tree
/// [2]: https://en.wikipedia.org/wiki/Graph_reduction
#[cfg(feature = "std")]
pub fn hash_vec_merkle_tree(vec: Vec<Digest>) -> Digest {
    vec.into_iter()
        .tree_fold1(|x, y| Digest::hash_pair(&x, &y))
        .unwrap_or(SENTINEL2)
}

/// Hashes a [BTreeMap].
#[cfg(feature = "std")]
pub fn hash_btree_map<K, V>(btree_map: &BTreeMap<K, V>) -> Result<Digest, bytesrepr::Error>
where
    K: ToBytes,
    V: ToBytes,
{
    let mut kv_hashes: Vec<Digest> = Vec::with_capacity(btree_map.len());
    for (key, value) in btree_map.iter() {
        kv_hashes.push(Digest::hash_pair(
            &Digest::hash(key.to_bytes()?),
            &Digest::hash(value.to_bytes()?),
        ))
    }
    Ok(hash_vec_merkle_tree(kv_hashes))
}

/// Hashes a `&[Digest]` using a [right fold][1].
///
/// This pattern of hashing is as follows:
///
/// ```text
/// hash_pair(a, &hash_pair(b, &hash_pair(c, &SENTINEL)))
/// ```
///
/// Unlike Merkle trees, this is suited to hashing heterogeneous lists we may wish to extend in the
/// future (ie, hashes of data structures that may undergo revision).
///
/// Returns [`SENTINEL1`] when given an empty [`Vec`] as input.
///
/// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
#[cfg(feature = "std")]
pub fn hash_slice_rfold(slice: &[Digest]) -> Digest {
    hash_slice_with_proof(slice, SENTINEL1)
}

/// Hashes a `&[Digest]` using a [right fold][1]. Uses `proof` as a Merkle proof for the missing
/// tail of the slice.
///
/// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
#[cfg(feature = "std")]
pub fn hash_slice_with_proof(slice: &[Digest], proof: Digest) -> Digest {
    slice
        .iter()
        .rfold(proof, |prev, next| Digest::hash_pair(next, &prev))
}

#[cfg(test)]
mod test {
    use super::*;

    use std::iter;

    use proptest_attr_macro::proptest;

    #[proptest]
    fn bytesrepr_roundtrip(data: [u8; Digest::LENGTH]) {
        let hash = Digest(data);
        bytesrepr::test_serialization_roundtrip(&hash);
    }

    #[test]
    fn blake2b_hash_known() {
        let inputs_and_digests = [
            (
                "",
                "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
            ),
            (
                "abc",
                "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319",
            ),
            (
                "The quick brown fox jumps over the lazy dog",
                "01718cec35cd3d796dd00020e0bfecb473ad23457d063b75eff29c0ffa2e58a9",
            ),
        ];
        for (known_input, expected_digest) in &inputs_and_digests {
            let known_input: &[u8] = known_input.as_ref();
            assert_eq!(*expected_digest, format!("{:?}", Digest::hash(known_input)));
        }
    }

    #[test]
    fn from_valid_hex_should_succeed() {
        for char in "abcdefABCDEF0123456789".chars() {
            let input: String = iter::repeat(char).take(64).collect();
            assert!(Digest::from_hex(input).is_ok());
        }
    }

    #[test]
    fn from_hex_invalid_length_should_fail() {
        for len in &[2_usize, 62, 63, 65, 66] {
            let input: String = "f".repeat(*len);
            assert!(Digest::from_hex(input).is_err());
        }
    }

    #[test]
    fn from_hex_invalid_char_should_fail() {
        for char in "g %-".chars() {
            let input: String = iter::repeat('f').take(63).chain(iter::once(char)).collect();
            assert!(Digest::from_hex(input).is_err());
        }
    }

    #[test]
    fn should_display_digest_in_hex() {
        let hash = Digest([0u8; 32]);
        let hash_hex = format!("{:?}", hash);
        assert_eq!(
            hash_hex,
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn should_print_digest_lower_hex() {
        let hash = Digest([10u8; 32]);
        let hash_lower_hex = format!("{:x}", hash);
        assert_eq!(
            hash_lower_hex,
            "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
        )
    }

    #[test]
    fn should_print_digest_upper_hex() {
        let hash = Digest([10u8; 32]);
        let hash_upper_hex = format!("{:X}", hash);
        assert_eq!(
            hash_upper_hex,
            "0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A"
        )
    }

    #[test]
    fn alternate_should_prepend_0x() {
        let hash = Digest([0u8; 32]);
        let hash_hex_alt = format!("{:#x}", hash);
        assert_eq!(
            hash_hex_alt,
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        )
    }

    #[test]
    fn test_hash_pair() {
        let hash1 = Digest([1u8; 32]);
        let hash2 = Digest([2u8; 32]);

        let hash = Digest::hash_pair(&hash1, &hash2);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "30b600fb1f0cc0b3f0fc28cdcb7389405a6659be81c7d5c5905725aa3a5119ce"
        );
    }

    #[test]
    fn test_hash_rfold() {
        let hashes = vec![
            Digest([1u8; 32]),
            Digest([2u8; 32]),
            Digest([3u8; 32]),
            Digest([4u8; 32]),
            Digest([5u8; 32]),
        ];

        let hash = hash_slice_rfold(&hashes[..]);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "e137f4eb94d2387065454eecfe2cdb5584e3dbd5f1ca07fc511fffd13d234e8e"
        );

        let proof = hash_slice_rfold(&hashes[2..]);
        let hash_proof = hash_slice_with_proof(&hashes[..2], proof);

        assert_eq!(hash, hash_proof);
    }

    #[test]
    fn test_hash_merkle_odd() {
        let hashes = vec![
            Digest([1u8; 32]),
            Digest([2u8; 32]),
            Digest([3u8; 32]),
            Digest([4u8; 32]),
            Digest([5u8; 32]),
        ];

        let hash = hash_vec_merkle_tree(hashes);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "c18aaf359f7b4643991f68fbfa8c503eb460da497399cdff7d8a2b1bc4399589"
        );
    }

    #[test]
    fn test_hash_merkle_even() {
        let hashes = vec![
            Digest([1u8; 32]),
            Digest([2u8; 32]),
            Digest([3u8; 32]),
            Digest([4u8; 32]),
            Digest([5u8; 32]),
            Digest([6u8; 32]),
        ];

        let hash = hash_vec_merkle_tree(hashes);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "0470ecc8abdcd6ecd3a4c574431b80bb8751c7a43337d5966dadf07899f8804b"
        );
    }

    #[test]
    fn test_hash_btreemap() {
        let mut map = BTreeMap::new();
        let _ = map.insert(Digest([1u8; 32]), Digest([2u8; 32]));
        let _ = map.insert(Digest([3u8; 32]), Digest([4u8; 32]));
        let _ = map.insert(Digest([5u8; 32]), Digest([6u8; 32]));
        let _ = map.insert(Digest([7u8; 32]), Digest([8u8; 32]));
        let _ = map.insert(Digest([9u8; 32]), Digest([10u8; 32]));

        let hash = hash_btree_map(&map).unwrap();
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "f3bc94beb2470d5c09f575b439d5f238bdc943233774c7aa59e597cc2579e148"
        );
    }
}
