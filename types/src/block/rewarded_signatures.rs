use alloc::{collections::BTreeSet, vec::Vec};

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    PublicKey,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

use serde::{Deserialize, Serialize};
use tracing::error;

/// Describes finality signatures that will be rewarded in a block. Consists of a vector of
/// `SingleBlockRewardedSignatures`, each of which describes signatures for a single ancestor
/// block. The first entry represents the signatures for the parent block, the second for the
/// parent of the parent, and so on.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct RewardedSignatures(Vec<SingleBlockRewardedSignatures>);

/// List of identifiers for finality signatures for a particular past block.
///
/// That past block height is current_height - signature_rewards_max_delay, the latter being defined
/// in the chainspec.
///
/// We need to wait for a few blocks to pass (`signature_rewards_max_delay`) to store the finality
/// signers because we need a bit of time to get the block finality.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct SingleBlockRewardedSignatures(Vec<u8>);

impl SingleBlockRewardedSignatures {
    /// Creates a new set of recorded finality signaures from the era's validators +
    /// the list of validators which signed.
    pub fn from_validator_set<'a>(
        public_keys: &BTreeSet<PublicKey>,
        all_validators: impl IntoIterator<Item = &'a PublicKey>,
    ) -> Self {
        // Take the validators list
        // Replace the ones who signed with 1 and the ones who didn't with 0
        // Pack everything into bytes
        let result = Self::pack(
            all_validators
                .into_iter()
                .map(|key| u8::from(public_keys.contains(key))),
        );

        let included_count: u32 = result.0.iter().map(|c| c.count_ones()).sum();
        if included_count as usize != public_keys.len() {
            error!(
                included_count,
                expected_count = public_keys.len(),
                "error creating past finality signatures from validator set"
            );
        }

        result
    }

    /// Gets the list of validators which signed from a set of recorded finality signaures (`self`)
    /// + the era's validators.
    pub fn to_validator_set(
        &self,
        all_validators: impl IntoIterator<Item = PublicKey>,
    ) -> BTreeSet<PublicKey> {
        self.unpack()
            .zip(all_validators.into_iter())
            .filter_map(|(active, validator)| (active != 0).then_some(validator))
            .collect()
    }

    /// Packs the bits to bytes, to create a `PastFinalitySignature`
    /// from an iterator of bits.
    ///
    /// If a value is neither 1 nor 0, it is interpreted as a 1.
    #[doc(hidden)]
    pub fn pack(bits: impl Iterator<Item = u8>) -> Self {
        //use itertools::Itertools;

        fn set_bit_at(value: u8, position: usize) -> u8 {
            // Sanitize the value (must be 0 or 1):
            let value = u8::from(value != 0);

            value << (7 - position)
        }

        let inner = chunks_8(bits)
            .map(|bits_chunk| {
                bits_chunk
                    .enumerate()
                    .fold(0, |acc, (pos, value)| acc | set_bit_at(value, pos))
            })
            .collect();

        SingleBlockRewardedSignatures(inner)
    }

    /// Unpacks the bytes to bits,
    /// to get a human readable representation of `PastFinalitySignature`.
    #[doc(hidden)]
    pub fn unpack(&self) -> impl Iterator<Item = u8> + '_ {
        // Returns the bit at the given position (0 or 1):
        fn bit_at(byte: u8, position: u8) -> u8 {
            (byte & (0b1000_0000 >> position)) >> (7 - position)
        }

        self.0
            .iter()
            .flat_map(|&byte| (0..8).map(move |i| bit_at(byte, i)))
    }

    /// Calculates the set difference of two instances of `SingleBlockRewardedSignatures`.
    #[doc(hidden)]
    pub fn difference(mut self, other: &SingleBlockRewardedSignatures) -> Self {
        for (self_byte, other_byte) in self.0.iter_mut().zip(other.0.iter()) {
            *self_byte &= !other_byte;
        }
        self
    }

    /// Calculates the set intersection of two instances of `SingleBlockRewardedSignatures`.
    pub fn intersection(mut self, other: &SingleBlockRewardedSignatures) -> Self {
        self.0 = self
            .0
            .iter()
            .zip(other.0.iter())
            .map(|(a, b)| *a & *b)
            .collect();
        self
    }

    /// Returns `true` if the set contains at least one signature.
    pub fn has_some(&self) -> bool {
        self.0.iter().any(|byte| *byte != 0)
    }
}

impl ToBytes for SingleBlockRewardedSignatures {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(Bytes::from(self.0.as_ref()).to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for SingleBlockRewardedSignatures {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, rest) = Bytes::from_bytes(bytes)?;
        Ok((SingleBlockRewardedSignatures(inner.into()), rest))
    }
}

impl RewardedSignatures {
    /// Creates a new instance of `RewardedSignatures`.
    pub fn new<I: IntoIterator<Item = SingleBlockRewardedSignatures>>(
        single_block_signatures: I,
    ) -> Self {
        Self(single_block_signatures.into_iter().collect())
    }

    /// Creates an instance of `RewardedSignatures` based on its unpacked (one byte per validator)
    /// representation.
    pub fn pack(unpacked: Vec<Vec<u8>>) -> Self {
        Self(
            unpacked
                .into_iter()
                .map(|single_block_signatures| {
                    SingleBlockRewardedSignatures::pack(single_block_signatures.into_iter())
                })
                .collect(),
        )
    }

    /// Creates an unpacked (one byte per validator) representation of the finality signatures to
    /// be rewarded in this block.
    pub fn unpack(&self) -> Vec<Vec<u8>> {
        self.0
            .iter()
            .map(|single_block_signatures| single_block_signatures.unpack().collect())
            .collect()
    }

    /// Returns this instance of `RewardedSignatures` with `num_blocks` of empty signatures
    /// prepended.
    pub fn left_padded(self, num_blocks: usize) -> Self {
        Self(
            core::iter::repeat_with(SingleBlockRewardedSignatures::default)
                .take(num_blocks)
                .chain(self.0)
                .collect(),
        )
    }

    /// Calculates the set difference between two instances of `RewardedSignatures`.
    pub fn difference(self, other: &RewardedSignatures) -> Self {
        Self(
            self.0
                .into_iter()
                .zip(other.0.iter())
                .map(|(single_block_signatures, other_block_signatures)| {
                    single_block_signatures.difference(other_block_signatures)
                })
                .collect(),
        )
    }

    /// Calculates the set intersection between two instances of `RewardedSignatures`.
    pub fn intersection(&self, other: &RewardedSignatures) -> Self {
        Self(
            self.0
                .iter()
                .zip(other.0.iter())
                .map(|(single_block_signatures, other_block_signatures)| {
                    single_block_signatures
                        .clone()
                        .intersection(other_block_signatures)
                })
                .collect(),
        )
    }

    /// Iterates over the `SingleBlockRewardedSignatures` for each rewarded block.
    pub fn iter(&self) -> impl Iterator<Item = &SingleBlockRewardedSignatures> {
        self.0.iter()
    }

    /// Iterates over the `SingleBlockRewardedSignatures`, yielding the signatures together with
    /// the block height for each entry. `block_height` is the height of the block that contains
    /// this instance of `RewardedSignatures`.
    pub fn iter_with_height(
        &self,
        block_height: u64,
    ) -> impl Iterator<Item = (u64, &SingleBlockRewardedSignatures)> {
        self.0.iter().enumerate().map(move |(rel_height, sbrs)| {
            (
                block_height
                    .saturating_sub(rel_height as u64)
                    .saturating_sub(1),
                sbrs,
            )
        })
    }

    /// Returns `true` if there is at least one cited signature.
    pub fn has_some(&self) -> bool {
        self.0.iter().any(|signatures| signatures.has_some())
    }
}

pub(crate) static EMPTY: RewardedSignatures = RewardedSignatures(Vec::new());

impl ToBytes for RewardedSignatures {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for RewardedSignatures {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Vec::<SingleBlockRewardedSignatures>::from_bytes(bytes)
            .map(|(inner, rest)| (RewardedSignatures(inner), rest))
    }
}

/// Chunks an iterator over `u8`s into pieces of maximum size of 8.
fn chunks_8(bits: impl Iterator<Item = u8>) -> impl Iterator<Item = impl Iterator<Item = u8>> {
    struct Chunks<B>(B);

    struct Chunk {
        values: [u8; 8],
        index: usize,
        max: usize,
    }

    impl<B> Iterator for Chunks<B>
    where
        B: Iterator<Item = u8>,
    {
        type Item = Chunk;

        fn next(&mut self) -> Option<Self::Item> {
            let mut values = [0; 8];
            let max = core::iter::zip(&mut values, &mut self.0)
                .map(|(array_slot, value)| *array_slot = value)
                .count();

            (max != 0).then_some(Chunk {
                values,
                max,
                index: 0,
            })
        }
    }

    impl Iterator for Chunk {
        type Item = u8;

        fn next(&mut self) -> Option<Self::Item> {
            if self.index < self.max {
                let n = self.values.get(self.index).cloned();
                self.index += 1;
                n
            } else {
                None
            }
        }
    }

    Chunks(bits)
}

#[cfg(test)]
mod tests {
    use super::{chunks_8, SingleBlockRewardedSignatures};
    use crate::{
        bytesrepr::{FromBytes, ToBytes},
        testing::TestRng,
        PublicKey,
    };
    use rand::{seq::IteratorRandom, Rng};
    use std::collections::BTreeSet;

    #[test]
    fn empty_signatures() {
        let rng = &mut TestRng::new();
        let validators: Vec<_> = std::iter::repeat_with(|| PublicKey::random(rng))
            .take(7)
            .collect();
        let original_signed = BTreeSet::new();

        let past_finality_signatures =
            SingleBlockRewardedSignatures::from_validator_set(&original_signed, validators.iter());

        assert_eq!(past_finality_signatures.0, &[0]);

        let signed = past_finality_signatures.to_validator_set(validators.into_iter());

        assert_eq!(original_signed, signed);
    }

    #[test]
    fn from_and_to_methods_match_in_a_simple_case() {
        let rng = &mut TestRng::new();
        let validators: Vec<_> = std::iter::repeat_with(|| PublicKey::random(rng))
            .take(11)
            .collect();
        let signed = {
            let mut signed = BTreeSet::new();
            signed.insert(validators[2].clone());
            signed.insert(validators[5].clone());
            signed.insert(validators[6].clone());
            signed.insert(validators[8].clone());
            signed.insert(validators[10].clone());
            signed
        };

        let past_finality_signatures =
            SingleBlockRewardedSignatures::from_validator_set(&signed, validators.iter());

        assert_eq!(past_finality_signatures.0, &[0b0010_0110, 0b1010_0000]);

        let signed_ = past_finality_signatures.to_validator_set(validators.into_iter());

        assert_eq!(signed, signed_);
    }

    #[test]
    fn simple_serialization_roundtrip() {
        let data = SingleBlockRewardedSignatures(vec![1, 2, 3, 4, 5]);

        let serialized = data.to_bytes().unwrap();
        assert_eq!(serialized.len(), data.0.len() + 4);
        assert_eq!(data.serialized_length(), data.0.len() + 4);

        let (deserialized, rest) = SingleBlockRewardedSignatures::from_bytes(&serialized).unwrap();

        assert_eq!(data, deserialized);
        assert_eq!(rest, &[0u8; 0]);
    }

    #[test]
    fn serialization_roundtrip_of_empty_data() {
        let data = SingleBlockRewardedSignatures::default();

        let serialized = data.to_bytes().unwrap();
        assert_eq!(serialized, &[0; 4]);
        assert_eq!(data.serialized_length(), 4);

        let (deserialized, rest) = SingleBlockRewardedSignatures::from_bytes(&serialized).unwrap();

        assert_eq!(data, deserialized);
        assert_eq!(rest, &[0u8; 0]);
    }

    #[test]
    fn serialization_roundtrip_of_random_data() {
        let rng = &mut TestRng::new();
        let n_validators = rng.gen_range(50..200);
        let all_validators: BTreeSet<_> = std::iter::repeat_with(|| PublicKey::random(rng))
            .take(n_validators)
            .collect();
        let n_to_sign = rng.gen_range(0..all_validators.len());
        let public_keys = all_validators
            .iter()
            .cloned()
            .choose_multiple(rng, n_to_sign)
            .into_iter()
            .collect();

        let past_finality_signatures =
            SingleBlockRewardedSignatures::from_validator_set(&public_keys, all_validators.iter());

        let serialized = past_finality_signatures.to_bytes().unwrap();
        let (deserialized, rest) = SingleBlockRewardedSignatures::from_bytes(&serialized).unwrap();

        assert_eq!(public_keys, deserialized.to_validator_set(all_validators));
        assert_eq!(rest, &[0u8; 0]);
    }

    #[test]
    fn chunk_iterator() {
        fn v(maybe_chunk: Option<impl Iterator<Item = u8>>) -> Option<Vec<u8>> {
            maybe_chunk.map(itertools::Itertools::collect_vec)
        }

        // Empty chunks:

        let mut chunks = chunks_8(IntoIterator::into_iter([]));

        assert_eq!(v(chunks.next()), None);

        // Exact size chunk:

        let mut chunks = chunks_8(IntoIterator::into_iter([10, 11, 12, 13, 14, 15, 16, 17]));

        assert_eq!(v(chunks.next()), Some(vec![10, 11, 12, 13, 14, 15, 16, 17]));
        assert_eq!(v(chunks.next()), None);

        // Chunks with a remainder:

        let mut chunks = chunks_8(IntoIterator::into_iter([
            10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
        ]));

        assert_eq!(v(chunks.next()), Some(vec![10, 11, 12, 13, 14, 15, 16, 17]));
        assert_eq!(v(chunks.next()), Some(vec![18, 19, 20, 21, 22, 23, 24, 25]));
        assert_eq!(v(chunks.next()), Some(vec![26]));
    }
}

#[cfg(any(feature = "testing", test))]
impl SingleBlockRewardedSignatures {
    /// Returns random data.
    pub fn random(rng: &mut crate::testing::TestRng, n_validators: usize) -> Self {
        let mut bytes = vec![0; (n_validators + 7) / 8];

        rand::RngCore::fill_bytes(rng, bytes.as_mut());

        SingleBlockRewardedSignatures(bytes)
    }
}
