//! A library providing hashing functionality including Merkle Proof utilities.

#![doc(html_root_url = "https://docs.rs/casper-hashing/1.0.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]

use std::{
    array::TryFromSliceError,
    collections::BTreeMap,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter, LowerHex, UpperHex},
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use itertools::Itertools;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

/// The output of the hash function.
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
    /// The number of bytes in a `Digest`.
    pub const LENGTH: usize = 32;

    /// Sentinel hash to be used for hashing options in the case of `None`.
    pub const SENTINEL_NONE: Digest = Digest([0u8; Digest::LENGTH]);
    /// Sentinel hash to be used by `hash_slice_rfold`. Terminates the fold.
    pub const SENTINEL_RFOLD: Digest = Digest([1u8; Digest::LENGTH]);
    /// Sentinel hash to be used by `hash_vec_merkle_tree` in the case of an empty list.
    pub const SENTINEL_MERKLE_TREE: Digest = Digest([2u8; Digest::LENGTH]);

    /// Creates a 32-byte BLAKE2b hash digest from a given a piece of data
    pub fn hash<T: AsRef<[u8]>>(data: T) -> Digest {
        let mut ret = [0u8; Digest::LENGTH];
        // NOTE: Safe to unwrap here because our digest length is constant and valid
        let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
        hasher.update(data);
        hasher.finalize_variable(|hash| ret.clone_from_slice(hash));
        Digest(ret)
    }

    /// Hashes a pair of byte slices.
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

    /// Returns the underlying BLAKE2b hash bytes
    pub fn value(&self) -> [u8; Digest::LENGTH] {
        self.0
    }

    /// Converts the underlying BLAKE2b hash digest array to a `Vec`
    pub fn into_vec(self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Hashes a `Vec` of `Digest`s into a single `Digest` by constructing a [Merkle tree][1].
    /// Reduces pairs of elements in the `Vec` by repeatedly calling [`Digest::hash_pair`]. This
    /// hash procedure is suited to hashing `BTree`s.
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
    /// Returns [`Digest::SENTINEL_MERKLE_TREE`] when the input is empty.
    ///
    /// [1]: https://en.wikipedia.org/wiki/Merkle_tree
    /// [2]: https://en.wikipedia.org/wiki/Graph_reduction
    pub fn hash_vec_merkle_tree(vec: Vec<Digest>) -> Digest {
        vec.into_iter()
            .tree_fold1(|x, y| Digest::hash_pair(&x, &y))
            .unwrap_or(Self::SENTINEL_MERKLE_TREE)
    }

    /// Hashes a `BTreeMap`.
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
        Ok(Self::hash_vec_merkle_tree(kv_hashes))
    }

    /// Hashes a `&[Digest]` using a [right fold][1].
    ///
    /// This pattern of hashing is as follows:
    ///
    /// ```text
    /// hash_pair(a, &hash_pair(b, &hash_pair(c, &SENTINEL_RFOLD)))
    /// ```
    ///
    /// Unlike Merkle trees, this is suited to hashing heterogeneous lists we may wish to extend in
    /// the future (ie, hashes of data structures that may undergo revision).
    ///
    /// Returns [`Digest::SENTINEL_RFOLD`] when given an empty slice as input.
    ///
    /// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
    pub fn hash_slice_rfold(slice: &[Digest]) -> Digest {
        Self::hash_slice_with_proof(slice, Self::SENTINEL_RFOLD)
    }

    /// Hashes a `&[Digest]` using a [right fold][1]. Uses `proof` as a Merkle proof for the
    /// missing tail of the slice.
    ///
    /// [1]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)#Linear_folds
    pub fn hash_slice_with_proof(slice: &[Digest], proof: Digest) -> Digest {
        slice
            .iter()
            .rfold(proof, |prev, next| Digest::hash_pair(next, &prev))
    }
}

impl LowerHex for Digest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let hex_string = base16::encode_lower(&self.value());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

impl UpperHex for Digest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let hex_string = base16::encode_upper(&self.value());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

impl Display for Digest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:10}", base16::encode_lower(self))
    }
}

impl Debug for Digest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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

impl ToBytes for Digest {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Digest {
    #[inline(always)]
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        FromBytes::from_bytes(bytes).map(|(arr, rem)| (Digest(arr), rem))
    }
}

impl hex::FromHex for Digest {
    type Error = hex::FromHexError;

    fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        Ok(Digest(hex::FromHex::from_hex(hex)?))
    }
}

#[cfg(test)]
mod test {
    use std::iter;

    use hex::FromHex;
    use proptest_attr_macro::proptest;

    use super::*;

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

        let hash = Digest::hash_slice_rfold(&hashes[..]);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "e137f4eb94d2387065454eecfe2cdb5584e3dbd5f1ca07fc511fffd13d234e8e"
        );

        let proof = Digest::hash_slice_rfold(&hashes[2..]);
        let hash_proof = Digest::hash_slice_with_proof(&hashes[..2], proof);

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

        let hash = Digest::hash_vec_merkle_tree(hashes);
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

        let hash = Digest::hash_vec_merkle_tree(hashes);
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

        let hash = Digest::hash_btree_map(&map).unwrap();
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "f3bc94beb2470d5c09f575b439d5f238bdc943233774c7aa59e597cc2579e148"
        );
    }
}
