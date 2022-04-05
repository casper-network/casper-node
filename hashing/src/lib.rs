//! A library providing hashing functionality including Merkle Proof utilities.
#![doc(html_root_url = "https://docs.rs/casper-hashing/1.4.3")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]

mod chunk_with_proof;
pub mod error;
mod indexed_merkle_proof;

use std::{
    array::TryFromSliceError,
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    fmt::{self, Debug, Display, Formatter, LowerHex, UpperHex},
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use itertools::Itertools;
use once_cell::sync::OnceCell;
#[cfg(test)]
use rand::{distributions::Standard, prelude::Distribution, Rng};
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex, CLType, CLTyped,
};
pub use chunk_with_proof::ChunkWithProof;
pub use error::MerkleConstructionError;

/// Possible hashing errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Incorrect digest length {0}, expected length {}.", Digest::LENGTH)]
    /// The digest length was an incorrect size.
    IncorrectDigestLength(usize),
    /// There was a decoding error.
    #[error("Base16 decode error {0}.")]
    Base16DecodeError(base16::DecodeError),
}

/// The output of the hash function.
#[derive(Copy, Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Hex-encoded hash digest.")]
pub struct Digest(#[schemars(skip, with = "String")] [u8; Digest::LENGTH]);

impl Digest {
    /// The number of bytes in a `Digest`.
    pub const LENGTH: usize = 32;

    /// Sentinel hash to be used for hashing options in the case of `None`.
    pub const SENTINEL_NONE: Digest = Digest([0u8; Digest::LENGTH]);
    /// Sentinel hash to be used by `hash_slice_rfold`. Terminates the fold.
    pub const SENTINEL_RFOLD: Digest = Digest([1u8; Digest::LENGTH]);
    /// Sentinel hash to be used by `hash_merkle_tree` in the case of an empty list.
    pub const SENTINEL_MERKLE_TREE: Digest = Digest([2u8; Digest::LENGTH]);

    /// Creates a 32-byte BLAKE2b hash digest from a given a piece of data.
    pub fn hash<T: AsRef<[u8]>>(data: T) -> Digest {
        Self::blake2b_hash(data)
    }

    /// Creates a 32-byte BLAKE2b hash digest from a given a piece of data
    pub(crate) fn blake2b_hash<T: AsRef<[u8]>>(data: T) -> Digest {
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

    /// Hashes a raw Merkle root and leaf count to firm the final Merkle hash.
    ///
    /// To avoid pre-image attacks, the final hash that is based upon the number of leaves in the
    /// Merkle tree and the root hash is prepended with a padding to ensure it is longer than the
    /// actual chunk size.
    ///
    /// Without this feature, an attacker could construct an item that is only a few bytes long but
    /// hashes to the same value as a much longer, chunked item by hashing `(len || root hash of
    /// longer item's Merkle tree root)`.
    ///
    /// This function computes the correct final hash by ensuring the hasher used has been
    /// initialized with padding before. For efficiency reasons it uses a memoized hasher state
    /// computed on first run and cloned afterwards.
    fn hash_merkle_root(leaf_count: u64, root: Digest) -> Digest {
        static PAIR_PREFIX_HASHER: OnceCell<VarBlake2b> = OnceCell::new();

        let mut result = [0; Digest::LENGTH];
        let mut hasher = PAIR_PREFIX_HASHER
            .get_or_init(|| {
                let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
                hasher.update(&[0u8; ChunkWithProof::CHUNK_SIZE_BYTES]);
                hasher
            })
            .clone();

        hasher.update(leaf_count.to_le_bytes());
        hasher.update(root);
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

    /// Hashes an `impl IntoIterator` of [`Digest`]s into a single [`Digest`] by
    /// constructing a [Merkle tree][1]. Reduces pairs of elements in the collection by repeatedly
    /// calling [Digest::hash_pair].
    ///
    /// The pattern of hashing is as follows. It is akin to [graph reduction][2]:
    ///
    /// ```text
    /// 1 2 4 5 8 9
    /// │ │ │ │ │ │
    /// └─3 └─6 └─10
    ///   │   │   │
    ///   └───7   │
    ///       │   │
    ///       └───11
    /// ```
    ///
    /// Finally hashes the number of elements with the resulting hash. In the example above the
    /// final output would be `hash_pair(6_u64.to_le_bytes(), l)`.
    ///
    /// Returns [`Digest::SENTINEL_MERKLE_TREE`] when the input is empty.
    ///
    /// [1]: https://en.wikipedia.org/wiki/Merkle_tree
    /// [2]: https://en.wikipedia.org/wiki/Graph_reduction
    pub fn hash_merkle_tree<I>(leaves: I) -> Digest
    where
        I: IntoIterator<Item = Digest>,
        I::IntoIter: ExactSizeIterator,
    {
        let leaves = leaves.into_iter();
        let leaf_count = leaves.len() as u64;

        leaves.tree_fold1(Digest::hash_pair).map_or_else(
            || Digest::SENTINEL_MERKLE_TREE,
            |raw_root| Digest::hash_merkle_root(leaf_count, raw_root),
        )
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
        Ok(Self::hash_merkle_tree(kv_hashes))
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

    /// Returns a `Digest` parsed from a hex-encoded `Digest`.
    pub fn from_hex<T: AsRef<[u8]>>(hex_input: T) -> Result<Self, Error> {
        let bytes = checksummed_hex::decode(&hex_input).map_err(Error::Base16DecodeError)?;
        let slice: [u8; Self::LENGTH] = bytes
            .try_into()
            .map_err(|_| Error::IncorrectDigestLength(hex_input.as_ref().len()))?;
        Ok(Digest(slice))
    }
}

impl CLTyped for Digest {
    fn cl_type() -> CLType {
        CLType::ByteArray(Digest::LENGTH as u32)
    }
}

#[cfg(test)]
impl Distribution<Digest> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Digest {
        Digest(rng.gen::<[u8; Digest::LENGTH]>())
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
        write!(f, "{:10}", base16::encode_lower(&self.0))
    }
}

impl Debug for Digest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
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

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for Digest {
    #[inline(always)]
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        FromBytes::from_bytes(bytes).map(|(arr, rem)| (Digest(arr), rem))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            base16::encode_lower(&self.0).serialize(serializer)
        } else {
            // This is to keep backwards compatibility with how HexForm encodes
            // byte arrays. HexForm treats this like a slice.
            (&self.0[..]).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            let bytes =
                checksummed_hex::decode(hex_string.as_bytes()).map_err(SerdeError::custom)?;
            let data =
                <[u8; Digest::LENGTH]>::try_from(bytes.as_ref()).map_err(SerdeError::custom)?;
            Ok(Digest::from(data))
        } else {
            let data = <Vec<u8>>::deserialize(deserializer)?;
            Digest::try_from(data.as_slice()).map_err(D::Error::custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, iter};

    use proptest_attr_macro::proptest;

    use casper_types::bytesrepr::{self, ToBytes};

    use crate::{ChunkWithProof, Digest};

    #[proptest]
    fn bytesrepr_roundtrip(data: [u8; Digest::LENGTH]) {
        let hash = Digest(data);
        bytesrepr::test_serialization_roundtrip(&hash);
    }

    #[proptest]
    fn serde_roundtrip(data: [u8; Digest::LENGTH]) {
        let original_hash = Digest(data);
        let serialized = serde_json::to_string(&original_hash).unwrap();
        let deserialized_hash: Digest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original_hash, deserialized_hash);
    }

    #[test]
    fn serde_custom_serialization() {
        let serialized = serde_json::to_string(&Digest::SENTINEL_RFOLD).unwrap();
        let expected = format!("\"{}\"", Digest::SENTINEL_RFOLD);
        assert_eq!(expected, serialized);
    }

    #[test]
    fn hash_known() {
        // Data of length less or equal to [ChunkWithProof::CHUNK_SIZE_BYTES]
        // are hashed using Blake2B algorithm.
        // Larger data are chunked and merkle tree hash is calculated.
        //
        // Please note that [ChunkWithProof::CHUNK_SIZE_BYTES] is `test` configuration
        // is smaller than in production, to allow testing with more chunks
        // with still reasonable time and memory consumption.
        //
        // See: [Digest::hash]
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
                "0123456789",
                "7b6cb8d374484e221785288b035dc53fc9ddf000607f473fc2a3258d89a70398",
            ),
            (
                "01234567890",
                "3d199478c18b7fe3ca1f4f2a9b3e07f708ff66ed52eb345db258abe8a812ed5c",
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

        let hash = Digest::hash_merkle_tree(hashes);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "775cec8133b97b0e8d4e97659025d5bac4ed7c8927d1bd99cf62114df57f3e74"
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

        let hash = Digest::hash_merkle_tree(hashes);
        let hash_lower_hex = format!("{:x}", hash);

        assert_eq!(
            hash_lower_hex,
            "4bd50b08a8366b28c35bc831b95d147123bad01c29ffbf854b659c4b3ea4086c"
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
            "fd1214a627473ffc6d6cc97e7012e6344d74abbf987b48cde5d0642049a0db98"
        );
    }

    #[test]
    fn digest_deserialize_regression() {
        let input = Digest([0; 32]);
        let serialized = bincode::serialize(&input).expect("failed to serialize.");

        let expected = vec![
            32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        assert_eq!(expected, serialized);
    }

    #[test]
    fn should_assert_simple_digest_serialization_format() {
        let digest_bytes = [0; 32];

        assert_eq!(
            Digest(digest_bytes).to_bytes().unwrap(),
            digest_bytes.to_vec()
        );
    }

    #[test]
    fn merkle_roots_are_preimage_resistent() {
        // Input data is two chunks long.
        //
        // The resulting tree will look like this:
        //
        // 1..0  a..j
        // │     │
        // └─────── R
        //
        // The merkle root is thus: R = h( h(1..0) || h(a..j) )
        //
        // h(1..0) = 807f1ba73147c3a96c2d63b38dd5a5f514f66290a1436bb9821e9f2a72eff263
        // h(a..j) = 499e1cdb476523fedafc9d9db31125e2744f271578ea95b16ab4bd1905f05fea
        // R=h(h(1..0)||h(a..j)) = 1319394a98d0cb194f960e3748baeb2045a9ec28aa51e0d42011be43f4a91f5f
        // h(2u64le || R) = c31f0bb6ef569354d1a26c3a51f1ad4b6d87cef7f73a290ab6be8db6a9c7d4ee
        //
        // The final step is to hash h(2u64le || R), which is the length as little endian
        // concatenated with the root.

        // Constants used here assume a chunk size of 10 bytes.
        assert_eq!(ChunkWithProof::CHUNK_SIZE_BYTES, 10);

        let long_data = b"1234567890abcdefghij";
        assert_eq!(long_data.len(), ChunkWithProof::CHUNK_SIZE_BYTES * 2);

        // The `long_data_hash` is constructed manually here, as `Digest::hash` still had
        // deactivated chunking code at the time this test was written.
        let long_data_hash = Digest::hash_merkle_tree(
            long_data
                .as_ref()
                .chunks(ChunkWithProof::CHUNK_SIZE_BYTES)
                .map(Digest::blake2b_hash),
        );

        // The concatenation of `2u64` in little endian + the Merkle root hash `R`. Note that this
        // is a valid hashable object on its own.
        let maybe_colliding_short_data = [
            2, 0, 0, 0, 0, 0, 0, 0, 19, 25, 57, 74, 152, 208, 203, 25, 79, 150, 14, 55, 72, 186,
            235, 32, 69, 169, 236, 40, 170, 81, 224, 212, 32, 17, 190, 67, 244, 169, 31, 95,
        ];

        // Use `blake2b_hash` to work around the issue of the chunk size being shorter than the
        // digest length.
        let short_data_hash = Digest::blake2b_hash(maybe_colliding_short_data);

        // Ensure there is no collision. You can verify this test is correct by temporarily changing
        // the `Digest::hash_merkle_tree` function to use the unpadded `hash_pair` function, instead
        // of `hash_merkle_root`.
        assert_ne!(long_data_hash, short_data_hash);

        // The expected input for the root hash is the colliding data, but prefixed with a full
        // chunk of zeros.
        let expected_final_hash_input = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 19, 25, 57, 74, 152, 208, 203,
            25, 79, 150, 14, 55, 72, 186, 235, 32, 69, 169, 236, 40, 170, 81, 224, 212, 32, 17,
            190, 67, 244, 169, 31, 95,
        ];
        assert_eq!(
            Digest::blake2b_hash(&expected_final_hash_input),
            long_data_hash
        );

        // Another way to specify this sanity check is to say that the short and long data should
        // hash differently.
        //
        // Note: This condition is true at the time of writing this test, where chunk hashing is
        //       disabled. It should still hold true once enabled.
        assert_ne!(
            Digest::hash(maybe_colliding_short_data),
            Digest::hash(long_data)
        );

        // In a similar manner, the internal padded data should also not hash equal to either, as it
        // should be hashed using the chunking function.
        assert_ne!(
            Digest::hash(maybe_colliding_short_data),
            Digest::hash(expected_final_hash_input)
        );
        assert_ne!(
            Digest::hash(long_data),
            Digest::hash(expected_final_hash_input)
        );
    }
}
