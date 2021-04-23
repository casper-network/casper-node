//! Cryptographic hash type and function.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter, LowerHex, UpperHex},
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use hex_fmt::HexFmt;
use itertools::Itertools;
#[cfg(test)]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::newtypes::Blake2bHash;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::Error;
#[cfg(test)]
use crate::testing::TestRng;
use std::collections::BTreeMap;

/// The hash digest; a wrapped `u8` array.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Default,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Hex-encoded hash digest.")]
pub struct Digest(
    #[serde(with = "HexForm::<[u8; Digest::LENGTH]>")]
    #[schemars(skip, with = "String")]
    [u8; Digest::LENGTH],
);

impl Digest {
    /// Length of `Digest` in bytes.
    pub const LENGTH: usize = 32;

    /// Returns a copy of the wrapped `u8` array.
    pub fn to_array(&self) -> [u8; Digest::LENGTH] {
        self.0
    }

    /// Returns a copy of the wrapped `u8` array as a `Vec`
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Returns a `Digest` parsed from a hex-encoded `Digest`.
    pub fn from_hex<T: AsRef<[u8]>>(hex_input: T) -> Result<Self, Error> {
        let mut inner = [0; Digest::LENGTH];
        hex::decode_to_slice(hex_input, &mut inner)?;
        Ok(Digest(inner))
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        Digest(rng.gen::<[u8; Digest::LENGTH]>())
    }
}

impl AsRef<[u8]> for Digest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<[u8; Digest::LENGTH]> for Digest {
    fn from(inner: [u8; Digest::LENGTH]) -> Self {
        Digest(inner)
    }
}

impl TryFrom<&[u8]> for Digest {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Digest, Self::Error> {
        <[u8; Digest::LENGTH]>::try_from(slice).map(Digest)
    }
}

impl Debug for Digest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", HexFmt(&self.0))
    }
}

impl Display for Digest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:10}", HexFmt(&self.0))
    }
}
impl LowerHex for Digest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        if formatter.alternate() {
            write!(formatter, "0x{}", HexFmt(&self.0))
        } else {
            write!(formatter, "{}", HexFmt(&self.0))
        }
    }
}

impl UpperHex for Digest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        if formatter.alternate() {
            write!(formatter, "0x{:X}", HexFmt(&self.0))
        } else {
            write!(formatter, "{:X}", HexFmt(&self.0))
        }
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
        FromBytes::from_bytes(bytes).map(|(inner, remainder)| (Digest(inner), remainder))
    }
}

/// Returns the hash of `data`.
pub fn hash<T: AsRef<[u8]>>(data: T) -> Digest {
    let mut result = [0; Digest::LENGTH];

    let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");
    hasher.update(data);
    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    Digest(result)
}

impl From<Digest> for Blake2bHash {
    fn from(digest: Digest) -> Self {
        let digest_bytes = digest.to_array();
        Blake2bHash::from(digest_bytes)
    }
}

impl From<Blake2bHash> for Digest {
    fn from(blake2bhash: Blake2bHash) -> Self {
        let bytes = blake2bhash.value();
        Digest::from(bytes)
    }
}

/// The sentinel value used for empty entries in Merkle proofs.
pub const SENTINEL: Digest = Digest([0; 32]);

/// Hashes a pair of [Digest]s.
pub fn hash_pair(hash1: &Digest, hash2: &Digest) -> Digest {
    let mut to_hash = [0; Digest::LENGTH * 2];
    to_hash[..Digest::LENGTH].copy_from_slice(&(hash1.to_array())[..]);
    to_hash[Digest::LENGTH..].copy_from_slice(&(hash2.to_array())[..]);
    hash(&to_hash)
}

/// Hashes a [Vec<Digest>] into a single [Digest] by constructing a [Merkle tree][1].
/// Reduces pairs of elements in the [Vec<Digest>] by repeatedly calling [hash_pair].
/// This hash procedure is suited to hashing [BTree]s.
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
/// Returns the empty [Digest] when the input is empty.
///
/// [1]: https://en.wikipedia.org/wiki/Merkle_tree
/// [2]: https://en.wikipedia.org/wiki/Graph_reduction
pub fn hash_vec_merkle_tree(vec: Vec<Digest>) -> Digest {
    vec.into_iter()
        .tree_fold1(|x, y| hash_pair(&x, &y))
        .unwrap_or(SENTINEL)
}

/// Hashes a [BTreeMap].
pub fn hash_btree_map<K, V>(btree_map: &BTreeMap<K, V>) -> Result<Digest, bytesrepr::Error>
where
    K: ToBytes,
    V: ToBytes,
{
    let mut kv_hashes: Vec<Digest> = Vec::with_capacity(btree_map.len());
    for (key, value) in btree_map.iter() {
        kv_hashes.push(hash_pair(&hash(key.to_bytes()?), &hash(value.to_bytes()?)))
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
/// Unlike Merkle trees, this is suited to hashing heterogeneous lists we may wish
/// to extend in the future (ie, hashes of data structures that may undergo revision).
///
/// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
pub fn hash_slice_rfold(slice: &[Digest]) -> Digest {
    slice
        .iter()
        .rfold(SENTINEL, |prev, next| hash_pair(next, &prev))
}

/// Hashes a `&[Digest]` using a [right fold][1]. Uses `proof` as a Merkle proof for the missing
/// tail of the slice.
///
/// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
pub fn hash_slice_with_proof(slice: &[Digest], proof: Digest) -> Digest {
    slice
        .iter()
        .rfold(proof, |prev, next| hash_pair(next, &prev))
}

#[cfg(test)]
mod test {
    use std::iter::{self, FromIterator};

    use super::*;

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
            assert_eq!(*expected_digest, format!("{:?}", hash(known_input)));
        }
    }

    #[test]
    fn from_valid_hex_should_succeed() {
        for char in "abcdefABCDEF0123456789".chars() {
            let input = String::from_iter(iter::repeat(char).take(64));
            assert!(Digest::from_hex(input).is_ok());
        }
    }

    #[test]
    fn from_hex_invalid_length_should_fail() {
        for len in &[2_usize, 62, 63, 65, 66] {
            let input = String::from_iter(iter::repeat('f').take(*len));
            assert!(Digest::from_hex(input).is_err());
        }
    }

    #[test]
    fn from_hex_invalid_char_should_fail() {
        for char in "g %-".chars() {
            let input = String::from_iter(iter::repeat('f').take(63).chain(iter::once(char)));
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
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let hash = Digest::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }

    #[test]
    fn test_hash_pair() {
        let hash1 = Digest([1u8; 32]);
        let hash2 = Digest([2u8; 32]);

        let hash = hash_pair(&hash1, &hash2);
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
            "aa0f8025b8548930b3a3b6b1912686eedd45c6655daeb1f5f9603a96740b0355"
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
