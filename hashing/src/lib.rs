//! A library providing hashing functionality including Merkle Proof utilities.
#![doc(html_root_url = "https://docs.rs/casper-hashing/1.4.2")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]

mod chunk_with_proof;
mod error;
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
#[cfg(test)]
use rand::{distributions::Standard, prelude::Distribution, Rng};
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex,
};
pub use chunk_with_proof::ChunkWithProof;

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
#[schemars(with = "String", description = "Checksummed hex-encoded hash digest.")]
pub struct Digest(#[schemars(skip, with = "String")] pub(crate) [u8; Digest::LENGTH]);

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
        // TODO:
        // Temporarily, to avoid potential regression, we always use the original hashing method.
        // After the `ChunkWithProof` is thoroughly tested we should replace
        // the current implementation with the commented one.
        // This change may require updating the hashes in the `test_hash_btreemap'
        // and `hash_known` tests.
        //
        // if Self::should_hash_with_chunks(&data) {
        //     Self::hash_merkle_tree(
        //         data.as_ref()
        //             .chunks(ChunkWithProof::CHUNK_SIZE_BYTES)
        //             .map(Self::blake2b_hash),
        //     )
        // } else {
        //     Self::blake2b_hash(data)
        // }
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

    // Temporarily unused, see comments inside `Digest::hash()` for details.
    #[allow(unused)]
    #[inline(always)]
    fn should_hash_with_chunks<T: AsRef<[u8]>>(data: T) -> bool {
        data.as_ref().len() > ChunkWithProof::CHUNK_SIZE_BYTES
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
    ///    │   │   │
    ///    └───7   │
    ///        │   │
    ///        └───11
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
        let leaf_count_bytes = (leaves.len() as u64).to_le_bytes();

        leaves.tree_fold1(Digest::hash_pair).map_or_else(
            || Digest::SENTINEL_MERKLE_TREE,
            |raw_root| Digest::hash_pair(leaf_count_bytes, raw_root),
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
        write!(f, "{}", checksummed_hex::encode(&self.0))
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
            checksummed_hex::encode(&self.0).serialize(serializer)
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

    use casper_types::bytesrepr;

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
                "0e5751c026E543B2E8AB2eB06099daA1d1e5df47778F7787FAaB45CDF12fe3A8",
            ),
            (
                "abc",
                "bDDd813C634239723171EF3FEE98579B94964e3bB1Cb3e427262c8C068D52319",
            ),
            (
                "0123456789",
                "7b6CB8D374484e221785288b035Dc53fC9dDf000607F473fc2a3258D89A70398",
            ),
            (
                "01234567890",
                "3D199478C18B7fE3ca1F4F2a9B3E07f708FF66ED52Eb345dB258ABE8a812eD5C",
            ),
            (
                "The quick brown fox jumps over the lazy dog",
                "01718CeC35cD3d796Dd00020E0bFeCB473ad23457d063b75EFf29c0fFA2E58A9",
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
            "9ba070a55c7f72600d7f3fee2c0a6e52ec89237d97a8677eb0612132fb34df60"
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
            "8768e1ca1b86ed3d722fb1b7d7a0228349c7d448058d0ce1e314f99dbd1c8573"
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
            "aae1660ca492ed9af6b2ead22f88b390aeb2ec0719654824d084aa6c6553ceeb"
        );
    }

    #[test]
    fn picks_correct_hashing_method() {
        let data_smaller_than_chunk_size = vec![];
        assert!(!Digest::should_hash_with_chunks(
            data_smaller_than_chunk_size
        ));

        let data_bigger_than_chunk_size = vec![0; ChunkWithProof::CHUNK_SIZE_BYTES * 2];
        assert!(Digest::should_hash_with_chunks(data_bigger_than_chunk_size));
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
}
