use std::collections::BTreeSet;

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    PublicKey,
};

use datasize::DataSize;
use itertools::Itertools;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List of identifiers for finality signatures for a particular past block.
#[derive(
    Clone,
    DataSize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Default,
)]
pub struct PastFinalitySignatures(Vec<u8>);

impl PastFinalitySignatures {
    #[allow(dead_code)] //TODO remove it when the code is used in a next ticket
    pub fn from_validator_set<'a>(
        public_keys: &BTreeSet<PublicKey>,
        all_validators: impl IntoIterator<Item = &'a PublicKey>,
    ) -> Self {
        let bits = all_validators
            .into_iter()
            .map(|key| public_keys.contains(key))
            .chunks(8)
            .into_iter()
            .map(|bits| {
                bits.enumerate()
                    .map(|(i, bit)| u8::from(bit) << (7 - i))
                    .fold(0, |acc, u| acc | u)
            })
            .collect();

        PastFinalitySignatures(bits)
    }

    #[allow(dead_code)] //TODO remove it when the code is used in a next ticket
    pub fn into_validator_set(
        self,
        all_validators: impl IntoIterator<Item = PublicKey>,
    ) -> BTreeSet<PublicKey> {
        fn active(bits: u8) -> [bool; 8] {
            [
                0 != bits & 0b1000_0000,
                0 != bits & 0b0100_0000,
                0 != bits & 0b0010_0000,
                0 != bits & 0b0001_0000,
                0 != bits & 0b0000_1000,
                0 != bits & 0b0000_0100,
                0 != bits & 0b0000_0010,
                0 != bits & 0b0000_0001,
            ]
        }

        self.0
            .into_iter()
            .zip(&all_validators.into_iter().chunks(8))
            .flat_map(|(u, validators_chunk)| {
                IntoIterator::into_iter(active(u))
                    .zip(validators_chunk)
                    .filter_map(|(on, validator)| on.then_some(validator))
            })
            .collect()
    }
}

impl ToBytes for PastFinalitySignatures {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(Bytes::from(self.0.as_ref()).to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for PastFinalitySignatures {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, rest) = Bytes::from_bytes(bytes)?;
        Ok((PastFinalitySignatures(inner.into()), rest))
    }
}

#[cfg(test)]
mod tests {
    use super::PastFinalitySignatures;
    use casper_types::{
        bytesrepr::{FromBytes, ToBytes},
        testing::TestRng,
        PublicKey,
    };
    use std::collections::BTreeSet;

    #[test]
    fn empty_signatures() {
        let rng = &mut TestRng::new();
        let validators: Vec<_> = std::iter::repeat_with(|| PublicKey::random(rng))
            .take(7)
            .collect();
        let original_signed = BTreeSet::new();

        let past_finality_signatures =
            PastFinalitySignatures::from_validator_set(&original_signed, validators.iter());

        assert_eq!(past_finality_signatures.0, &[0]);

        let signed = past_finality_signatures.into_validator_set(validators.into_iter());

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
            PastFinalitySignatures::from_validator_set(&signed, validators.iter());

        assert_eq!(past_finality_signatures.0, &[0b0010_0110, 0b1010_0000]);

        let signed_ = past_finality_signatures.into_validator_set(validators.into_iter());

        assert_eq!(signed, signed_);
    }

    #[test]
    fn simple_serialization_roundtrip() {
        let data = PastFinalitySignatures(vec![1, 2, 3, 4, 5]);

        let serialized = data.to_bytes().unwrap();
        assert_eq!(serialized.len(), data.0.len() + 4);
        assert_eq!(data.serialized_length(), data.0.len() + 4);

        let (deserialized, rest) = PastFinalitySignatures::from_bytes(&serialized).unwrap();

        assert_eq!(data, deserialized);
        assert_eq!(rest, &[0u8; 0]);
    }

    #[test]
    fn serialization_roundtrip_of_empty_data() {
        let data = PastFinalitySignatures::default();

        let serialized = data.to_bytes().unwrap();
        assert_eq!(serialized, &[0; 4]);
        assert_eq!(data.serialized_length(), 4);

        let (deserialized, rest) = PastFinalitySignatures::from_bytes(&serialized).unwrap();

        assert_eq!(data, deserialized);
        assert_eq!(rest, &[0u8; 0]);
    }

    #[test]
    fn serialization_roundtrip_of_random_data() {
        let rng = &mut TestRng::new();
        let data = PastFinalitySignatures::random(rng, 123);

        let serialized = data.to_bytes().unwrap();
        let (deserialized, rest) = PastFinalitySignatures::from_bytes(&serialized).unwrap();

        assert_eq!(data, deserialized);
        assert_eq!(rest, &[0u8; 0]);
    }
}

impl crate::utils::specimen::LargestSpecimen for PastFinalitySignatures {
    fn largest_specimen<E: crate::utils::specimen::SizeEstimator>(
        estimator: &E,
        _cache: &mut crate::utils::specimen::Cache,
    ) -> Self {
        PastFinalitySignatures(vec![
            u8::MAX;
            div_by_8_ceil(estimator.parameter("validator_count"))
        ])
    }
}

#[cfg(any(feature = "testing", test))]
impl PastFinalitySignatures {
    pub fn random(rng: &mut casper_types::testing::TestRng, n_validators: usize) -> Self {
        let mut bytes = vec![0; div_by_8_ceil(n_validators)];

        rand::RngCore::fill_bytes(rng, bytes.as_mut());

        PastFinalitySignatures(bytes)
    }
}

/// Divides by 8 but round to the higher integer.
fn div_by_8_ceil(n: usize) -> usize {
    n / 8 + usize::from(n % 8 != 0)
}
