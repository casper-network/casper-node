use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(feature = "json-schema")]
use serde_map_to_array::KeyValueJsonSchema;
use serde_map_to_array::{BTreeMapToArray, KeyValueLabels};

use super::{EraReport, JsonEraEnd};
#[cfg(feature = "json-schema")]
use crate::SecretKey;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, U512,
};

#[cfg(feature = "json-schema")]
static ERA_END: Lazy<EraEnd> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let next_era_validator_weights = {
        let mut next_era_validator_weights: BTreeMap<PublicKey, U512> = BTreeMap::new();
        next_era_validator_weights.insert(public_key_1, U512::from(123));
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([5u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(456),
        );
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([6u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(789),
        );
        next_era_validator_weights
    };

    let era_report = EraReport::example().clone();
    EraEnd::new(era_report, next_era_validator_weights)
});

/// Information related to the end of an era, and validator weights for the following era.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EraEnd {
    /// Equivocation, reward and validator inactivity information.
    pub(super) era_report: EraReport<PublicKey>,
    /// The validators for the upcoming era and their respective weights.
    #[serde(with = "BTreeMapToArray::<PublicKey, U512, NextEraValidatorLabels>")]
    pub(super) next_era_validator_weights: BTreeMap<PublicKey, U512>,
}

impl EraEnd {
    /// Returns equivocation, reward and validator inactivity information.
    pub fn era_report(&self) -> &EraReport<PublicKey> {
        &self.era_report
    }

    /// Returns the validators for the upcoming era and their respective weights.
    pub fn next_era_validator_weights(&self) -> &BTreeMap<PublicKey, U512> {
        &self.next_era_validator_weights
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn new(
        era_report: EraReport<PublicKey>,
        next_era_validator_weights: BTreeMap<PublicKey, U512>,
    ) -> Self {
        EraEnd {
            era_report,
            next_era_validator_weights,
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &ERA_END
    }
}

impl ToBytes for EraEnd {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.era_report.write_bytes(writer)?;
        self.next_era_validator_weights.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.era_report.serialized_length() + self.next_era_validator_weights.serialized_length()
    }
}

impl FromBytes for EraEnd {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_report, remainder) = EraReport::<PublicKey>::from_bytes(bytes)?;
        let (next_era_validator_weights, remainder) =
            BTreeMap::<PublicKey, U512>::from_bytes(remainder)?;
        let era_end = EraEnd {
            era_report,
            next_era_validator_weights,
        };
        Ok((era_end, remainder))
    }
}

impl Display for EraEnd {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "era end: {} ", self.era_report)
    }
}

struct NextEraValidatorLabels;

impl KeyValueLabels for NextEraValidatorLabels {
    const KEY: &'static str = "validator";
    const VALUE: &'static str = "weight";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for NextEraValidatorLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("ValidatorWeight");
    const JSON_SCHEMA_KV_DESCRIPTION: Option<&'static str> = Some(
        "A validator's public key paired with its weight, i.e. the total number of \
        motes staked by it and its delegators.",
    );
    const JSON_SCHEMA_KEY_DESCRIPTION: Option<&'static str> = Some("The validator's public key.");
    const JSON_SCHEMA_VALUE_DESCRIPTION: Option<&'static str> = Some("The validator's weight.");
}

#[cfg(all(feature = "std", feature = "json-schema"))]
impl From<JsonEraEnd> for EraEnd {
    fn from(json_data: JsonEraEnd) -> Self {
        let era_report = EraReport::from(json_data.era_report);
        let validator_weights = json_data
            .next_era_validator_weights
            .iter()
            .map(|validator_weight| (validator_weight.validator.clone(), validator_weight.weight))
            .collect();
        EraEnd::new(era_report, validator_weights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing::TestRng, BlockV2};

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block = {
            let mut block = BlockV2::random(rng);

            let next_era_weights = (0..6)
                .map(|index| (PublicKey::random(rng), U512::from(index)))
                .collect();
            block.header.era_end = Some(EraEnd::new(EraReport::random(rng), next_era_weights));

            block
        };

        bytesrepr::test_serialization_roundtrip(block.era_end().unwrap());
    }
}
