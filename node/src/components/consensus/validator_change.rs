use std::collections::{BTreeMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{PublicKey, U512};

use super::era_supervisor::Era;

/// A change to a validator's status between two eras.
#[derive(Serialize, Deserialize, Debug, JsonSchema, Eq, PartialEq)]
pub enum ValidatorChange {
    /// The validator got newly added to the validator set.
    Added,
    /// The validator was removed from the validator set.
    Removed,
    /// The validator was banned from this era.
    Banned,
    /// The validator was excluded from proposing new blocks in this era.
    CannotPropose,
    /// We saw the validator misbehave in this era.
    SeenAsFaulty,
}

impl ValidatorChange {
    pub(super) fn era_changes<I>(
        era0: &Era<I>,
        era1: &Era<I>,
    ) -> Vec<(PublicKey, ValidatorChange)> {
        let era0_metadata = EraMetadata::from(era0);
        let era1_metadata = EraMetadata::from(era1);
        Self::validator_changes(era0_metadata, era1_metadata)
    }

    fn validator_changes(
        era0_metadata: EraMetadata,
        era1_metadata: EraMetadata,
    ) -> Vec<(PublicKey, ValidatorChange)> {
        let mut changes = Vec::new();
        for pub_key in era0_metadata.validators.keys() {
            if !era1_metadata.validators.contains_key(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Removed));
            }
        }
        for pub_key in era1_metadata.seen_as_faulty {
            changes.push((pub_key.clone(), ValidatorChange::SeenAsFaulty));
        }
        for pub_key in era1_metadata.validators.keys() {
            if !era0_metadata.validators.contains_key(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Added));
            }
            if era1_metadata.faulty.contains(pub_key) && !era0_metadata.faulty.contains(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Banned));
            }
            if era1_metadata.cannot_propose.contains(pub_key)
                && !era0_metadata.cannot_propose.contains(pub_key)
            {
                changes.push((pub_key.clone(), ValidatorChange::CannotPropose));
            }
        }
        changes
    }
}

#[derive(Clone)]
struct EraMetadata<'a> {
    validators: &'a BTreeMap<PublicKey, U512>,
    seen_as_faulty: Vec<PublicKey>,
    faulty: &'a HashSet<PublicKey>,
    cannot_propose: &'a HashSet<PublicKey>,
}

impl<'a, I> From<&'a Era<I>> for EraMetadata<'a> {
    fn from(era: &'a Era<I>) -> Self {
        let seen_as_faulty = era
            .consensus
            .validators_with_evidence()
            .into_iter()
            .cloned()
            .collect();

        let validators = era.validators();
        let faulty = &era.faulty;
        let cannot_propose = &era.cannot_propose;
        Self {
            validators,
            seen_as_faulty,
            faulty,
            cannot_propose,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{crypto::AsymmetricKeyExt, testing::TestRng};

    fn preset_validators(rng: &mut TestRng) -> BTreeMap<PublicKey, U512> {
        let mut validators = BTreeMap::new();
        for i in 0..5 {
            validators.insert(PublicKey::random(rng), U512::from(i));
        }
        validators
    }

    #[test]
    fn should_report_added_status_change() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);
        let era0_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };
        // First, assert that adding a validator is being reported.
        let new_validator = PublicKey::random(&mut rng);
        let mut new_validator_set = validators.clone();
        new_validator_set.insert(new_validator.clone(), U512::from(9));

        let era1_metadata = EraMetadata {
            validators: &new_validator_set,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let expected_added_change = vec![(new_validator, ValidatorChange::Added)];
        let actual_change = ValidatorChange::validator_changes(era0_metadata, era1_metadata);

        assert_eq!(expected_added_change, actual_change);
    }

    #[test]
    fn should_report_removed_status_change() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);
        let removed_validator_key = PublicKey::random(&mut rng);
        let era0_validator_set = {
            let mut validators = validators.clone();
            validators.insert(removed_validator_key.clone(), U512::from(9));
            validators
        };
        let era0_metadata = EraMetadata {
            validators: &era0_validator_set,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };
        let era1_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let expected_change = vec![(removed_validator_key, ValidatorChange::Removed)];
        let actual_change = ValidatorChange::validator_changes(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change)
    }

    #[test]
    fn should_report_seen_as_faulty_status_change() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);
        let seen_as_faulty = PublicKey::random(&mut rng);
        let era0_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };
        let era1_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![seen_as_faulty.clone()],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let expected_change = vec![(seen_as_faulty, ValidatorChange::SeenAsFaulty)];
        let actual_change = ValidatorChange::validator_changes(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change)
    }

    #[test]
    fn should_report_banned_status_change() {
        let mut rng = crate::new_rng();
        let mut validators = preset_validators(&mut rng);

        let faulty_key = PublicKey::random(&mut rng);
        let mut faulty = HashSet::new();
        faulty.insert(faulty_key.clone());

        validators.insert(faulty_key.clone(), U512::from(9));

        let era0_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };
        let era1_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &faulty,
            cannot_propose: &Default::default(),
        };

        let expected_change = vec![(faulty_key, ValidatorChange::Banned)];
        let actual_change = ValidatorChange::validator_changes(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change)
    }

    #[test]
    fn should_report_cannot_propose_status_change() {
        let mut rng = crate::new_rng();
        let mut validators = preset_validators(&mut rng);

        let cannot_propose_key = PublicKey::random(&mut rng);
        let mut cannot_propose = HashSet::new();
        cannot_propose.insert(cannot_propose_key.clone());

        validators.insert(cannot_propose_key.clone(), U512::from(9));

        let era0_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let era1_metadata = EraMetadata {
            validators: &validators,
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &cannot_propose,
        };

        let expected_change = vec![(cannot_propose_key, ValidatorChange::CannotPropose)];
        let actual_change = ValidatorChange::validator_changes(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change)
    }
}
