use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt;

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
    DisplayIter, PublicKey, U512,
};

#[cfg(feature = "json-schema")]
static ERA_END_V2: Lazy<EraEndV2> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let secret_key_3 = SecretKey::ed25519_from_bytes([2; 32]).unwrap();
    let public_key_3 = PublicKey::from(&secret_key_3);

    let equivocators = vec![public_key_1.clone()];
    let inactive_validators = vec![public_key_3];
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
    let rewards = Default::default();

    EraEndV2::new(
        equivocators,
        inactive_validators,
        next_era_validator_weights,
        rewards,
    )
});

/// Information related to the end of an era, and validator weights for the following era.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EraEndV2 {
    /// The set of equivocators.
    pub(super) equivocators: Vec<PublicKey>,
    /// Validators that haven't produced any unit during the era.
    pub(super) inactive_validators: Vec<PublicKey>,
    /// The validators for the upcoming era and their respective weights.
    #[serde(with = "BTreeMapToArray::<PublicKey, U512, NextEraValidatorLabels>")]
    pub(super) next_era_validator_weights: BTreeMap<PublicKey, U512>,
    /// The rewards distributed to the validators.
    pub(super) rewards: BTreeMap<PublicKey, U512>,
}

impl EraEndV2 {
    /// Returns the set of equivocators.
    pub fn equivocators(&self) -> &[PublicKey] {
        &self.equivocators
    }

    /// Returns the validators that haven't produced any unit during the era.
    pub fn inactive_validators(&self) -> &[PublicKey] {
        &self.inactive_validators
    }

    /// Returns the validators for the upcoming era and their respective weights.
    pub fn next_era_validator_weights(&self) -> &BTreeMap<PublicKey, U512> {
        &self.next_era_validator_weights
    }

    /// Returns the rewards distributed to the validators.
    pub fn rewards(&self) -> &BTreeMap<PublicKey, U512> {
        &self.rewards
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn new(
        equivocators: Vec<PublicKey>,
        inactive_validators: Vec<PublicKey>,
        next_era_validator_weights: BTreeMap<PublicKey, U512>,
        rewards: BTreeMap<PublicKey, U512>,
    ) -> Self {
        EraEndV2 {
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            rewards,
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &ERA_END_V2
    }

    /// Returns a random `EraReport`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut crate::testing::TestRng) -> Self {
        use rand::Rng;

        let equivocators_count = rng.gen_range(0..5);
        let inactive_count = rng.gen_range(0..5);
        let next_era_validator_weights_count = rng.gen_range(0..5);
        let rewards_count = rng.gen_range(0..5);

        let equivocators = core::iter::repeat_with(|| PublicKey::random(rng))
            .take(equivocators_count)
            .collect();

        let inactive_validators = core::iter::repeat_with(|| PublicKey::random(rng))
            .take(inactive_count)
            .collect();

        let next_era_validator_weights = core::iter::repeat_with(|| {
            let pub_key = PublicKey::random(rng);
            let reward = rng.gen_range(1..=1_000_000_000);
            (pub_key, U512::from(reward))
        })
        .take(next_era_validator_weights_count)
        .collect();

        let rewards = core::iter::repeat_with(|| {
            let pub_key = PublicKey::random(rng);
            let reward = rng.gen_range(1..=1_000_000_000);
            (pub_key, U512::from(reward))
        })
        .take(rewards_count)
        .collect();

        Self::new(
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            rewards,
        )
    }
}

impl ToBytes for EraEndV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let EraEndV2 {
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            rewards,
        } = self;

        equivocators.write_bytes(writer)?;
        inactive_validators.write_bytes(writer)?;
        next_era_validator_weights.write_bytes(writer)?;
        rewards.write_bytes(writer)?;

        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let EraEndV2 {
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            rewards,
        } = self;

        equivocators.serialized_length()
            + inactive_validators.serialized_length()
            + next_era_validator_weights.serialized_length()
            + rewards.serialized_length()
    }
}

impl FromBytes for EraEndV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (equivocators, bytes) = Vec::from_bytes(bytes)?;
        let (inactive_validators, bytes) = Vec::from_bytes(bytes)?;
        let (next_era_validator_weights, bytes) = BTreeMap::from_bytes(bytes)?;
        let (rewards, bytes) = BTreeMap::from_bytes(bytes)?;
        let era_end = EraEndV2 {
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            rewards,
        };

        Ok((era_end, bytes))
    }
}

impl fmt::Display for EraEndV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let slashings = DisplayIter::new(&self.equivocators);
        let rewards = DisplayIter::new(
            self.rewards
                .iter()
                .map(|(public_key, amount)| format!("{}: {}", public_key, amount)),
        );

        write!(
            formatter,
            "era end: slash {}, reward {}",
            slashings, rewards
        )
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
