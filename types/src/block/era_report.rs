use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};
#[cfg(any(feature = "testing", test))]
use core::iter;

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

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "json-schema")]
use crate::SecretKey;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, DisplayIter, PublicKey,
};

#[cfg(feature = "json-schema")]
static ERA_REPORT: Lazy<EraReport<PublicKey>> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let equivocators = vec![public_key_1];

    let secret_key_3 = SecretKey::ed25519_from_bytes([2; 32]).unwrap();
    let public_key_3 = PublicKey::from(&secret_key_3);
    let inactive_validators = vec![public_key_3];

    let rewards = BTreeMap::new();

    EraReport {
        equivocators,
        rewards,
        inactive_validators,
    }
});

/// Equivocation, reward and validator inactivity information.
///
/// `VID` represents validator ID type, generally [`PublicKey`].
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(bound(
    serialize = "VID: Ord + Serialize",
    deserialize = "VID: Ord + Deserialize<'de>",
))]
#[cfg_attr(
    feature = "json-schema",
    schemars(description = "Equivocation, reward and validator inactivity information.")
)]
pub struct EraReport<VID> {
    /// The set of equivocators.
    pub(super) equivocators: Vec<VID>,
    /// Rewards for finalization of earlier blocks.
    #[serde(with = "BTreeMapToArray::<VID, u64, EraRewardsLabels>")]
    pub(super) rewards: BTreeMap<VID, u64>,
    /// Validators that haven't produced any unit during the era.
    pub(super) inactive_validators: Vec<VID>,
}

impl<VID> EraReport<VID> {
    /// Constructs a new `EraReport`.
    pub fn new(
        equivocators: Vec<VID>,
        rewards: BTreeMap<VID, u64>,
        inactive_validators: Vec<VID>,
    ) -> Self {
        EraReport {
            equivocators,
            rewards,
            inactive_validators,
        }
    }

    /// Returns the set of equivocators.
    pub fn equivocators(&self) -> &[VID] {
        &self.equivocators
    }

    /// Returns rewards for finalization of earlier blocks.
    ///
    /// This is a measure of the value of each validator's contribution to consensus, in
    /// fractions of the configured maximum block reward.
    pub fn rewards(&self) -> &BTreeMap<VID, u64> {
        &self.rewards
    }

    /// Returns validators that haven't produced any unit during the era.
    pub fn inactive_validators(&self) -> &[VID] {
        &self.inactive_validators
    }

    /// Returns a cryptographic hash of the `EraReport`.
    pub fn hash(&self) -> Digest
    where
        VID: ToBytes,
    {
        // Helper function to hash slice of validators
        fn hash_slice_of_validators<VID>(slice_of_validators: &[VID]) -> Digest
        where
            VID: ToBytes,
        {
            Digest::hash_merkle_tree(slice_of_validators.iter().map(|validator| {
                Digest::hash(validator.to_bytes().expect("Could not serialize validator"))
            }))
        }

        // Pattern match here leverages compiler to ensure every field is accounted for
        let EraReport {
            equivocators,
            inactive_validators,
            rewards,
        } = self;

        let hashed_equivocators = hash_slice_of_validators(equivocators);
        let hashed_inactive_validators = hash_slice_of_validators(inactive_validators);
        let hashed_rewards = Digest::hash_btree_map(rewards).expect("Could not hash rewards");

        Digest::hash_slice_rfold(&[
            hashed_equivocators,
            hashed_rewards,
            hashed_inactive_validators,
        ])
    }
}

impl<VID: Ord> Default for EraReport<VID> {
    fn default() -> Self {
        EraReport {
            equivocators: vec![],
            rewards: BTreeMap::new(),
            inactive_validators: vec![],
        }
    }
}

impl<VID: Display> Display for EraReport<VID> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let slashings = DisplayIter::new(&self.equivocators);
        let rewards = DisplayIter::new(
            self.rewards
                .iter()
                .map(|(public_key, amount)| format!("{}: {}", public_key, amount)),
        );
        write!(f, "era end: slash {}, reward {}", slashings, rewards)
    }
}

impl<VID: ToBytes> ToBytes for EraReport<VID> {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.equivocators.write_bytes(writer)?;
        self.rewards.write_bytes(writer)?;
        self.inactive_validators.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.equivocators.serialized_length()
            + self.rewards.serialized_length()
            + self.inactive_validators.serialized_length()
    }
}

impl<VID: FromBytes + Ord> FromBytes for EraReport<VID> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (equivocators, remainder) = Vec::<VID>::from_bytes(bytes)?;
        let (rewards, remainder) = BTreeMap::<VID, u64>::from_bytes(remainder)?;
        let (inactive_validators, remainder) = Vec::<VID>::from_bytes(remainder)?;
        let era_report = EraReport {
            equivocators,
            rewards,
            inactive_validators,
        };
        Ok((era_report, remainder))
    }
}

impl EraReport<PublicKey> {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &ERA_REPORT
    }

    /// Returns a random `EraReport`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use rand::Rng;

        let equivocators_count = rng.gen_range(0..5);
        let rewards_count = rng.gen_range(0..5);
        let inactive_count = rng.gen_range(0..5);
        let equivocators = iter::repeat_with(|| PublicKey::random(rng))
            .take(equivocators_count)
            .collect();
        let rewards = iter::repeat_with(|| {
            let pub_key = PublicKey::random(rng);
            let reward = rng.gen_range(1..(1_000_000_000 + 1));
            (pub_key, reward)
        })
        .take(rewards_count)
        .collect();
        let inactive_validators = iter::repeat_with(|| PublicKey::random(rng))
            .take(inactive_count)
            .collect();
        EraReport::new(equivocators, rewards, inactive_validators)
    }
}

struct EraRewardsLabels;

impl KeyValueLabels for EraRewardsLabels {
    const KEY: &'static str = "validator";
    const VALUE: &'static str = "amount";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for EraRewardsLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("EraReward");
    const JSON_SCHEMA_KV_DESCRIPTION: Option<&'static str> = Some(
        "A validator's public key paired with a measure of the value of its \
        contribution to consensus, as a fraction of the configured maximum block reward.",
    );
    const JSON_SCHEMA_KEY_DESCRIPTION: Option<&'static str> = Some("The validator's public key.");
    const JSON_SCHEMA_VALUE_DESCRIPTION: Option<&'static str> = Some("The reward amount.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let era_report = EraReport::random(rng);
        bytesrepr::test_serialization_roundtrip(&era_report);
    }
}
