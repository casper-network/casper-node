mod era_report;

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

#[cfg(feature = "json-schema")]
use crate::SecretKey;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, U512,
};
pub use era_report::EraReport;

#[cfg(feature = "json-schema")]
static ERA_END_V1: Lazy<EraEndV1> = Lazy::new(|| {
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
    EraEndV1::new(era_report, next_era_validator_weights)
});

/// Information related to the end of an era, and validator weights for the following era.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EraEndV1 {
    /// Equivocation, reward and validator inactivity information.
    pub(super) era_report: EraReport<PublicKey>,
    /// The validators for the upcoming era and their respective weights.
    #[serde(with = "BTreeMapToArray::<PublicKey, U512, NextEraValidatorLabels>")]
    pub(super) next_era_validator_weights: BTreeMap<PublicKey, U512>,
}

impl EraEndV1 {
    /// Returns equivocation, reward and validator inactivity information.
    pub fn era_report(&self) -> &EraReport<PublicKey> {
        &self.era_report
    }

    /// Retrieves the deploy hashes within the block.
    pub fn equivocators(&self) -> &[PublicKey] {
        self.era_report.equivocators()
    }

    /// Retrieves the transfer hashes within the block.
    pub fn inactive_validators(&self) -> &[PublicKey] {
        self.era_report.inactive_validators()
    }

    /// Retrieves the transfer hashes within the block.
    pub fn rewards(&self) -> &BTreeMap<PublicKey, u64> {
        self.era_report.rewards()
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
        EraEndV1 {
            era_report,
            next_era_validator_weights,
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &ERA_END_V1
    }
}

impl ToBytes for EraEndV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.era_report.write_bytes(writer)?;
        self.next_era_validator_weights.write_bytes(writer)?;

        Ok(())
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

impl FromBytes for EraEndV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_report, remainder) = EraReport::<PublicKey>::from_bytes(bytes)?;
        let (next_era_validator_weights, remainder) =
            BTreeMap::<PublicKey, U512>::from_bytes(remainder)?;
        let era_end = EraEndV1 {
            era_report,
            next_era_validator_weights,
        };
        Ok((era_end, remainder))
    }
}

impl Display for EraEndV1 {
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
