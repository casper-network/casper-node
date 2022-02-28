use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::PublicKey;

use super::era_supervisor::Era;

/// A change to a validator's status between two eras.
#[derive(Serialize, Deserialize, Debug, JsonSchema, Eq, PartialEq, Ord, PartialOrd)]
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

pub(super) struct ValidatorChanges(pub(super) Vec<(PublicKey, ValidatorChange)>);

impl ValidatorChanges {
    pub(super) fn new(era0: &Era, era1: &Era) -> Self {
        let era0_metadata = EraMetadata::from(era0);
        let era1_metadata = EraMetadata::from(era1);
        Self::new_from_metadata(era0_metadata, era1_metadata)
    }

    fn new_from_metadata(era0_metadata: EraMetadata, era1_metadata: EraMetadata) -> Self {
        // Validators in `era0` but not `era1` are labeled `Removed`.
        let removed_iter = era0_metadata
            .validators
            .difference(&era1_metadata.validators)
            .map(|&public_key| (public_key.clone(), ValidatorChange::Removed));

        // Validators in `era1` but not `era0` are labeled `Added`.
        let added_iter = era1_metadata
            .validators
            .difference(&era0_metadata.validators)
            .map(|&public_key| (public_key.clone(), ValidatorChange::Added));

        // Only those seen as faulty in `era1` are labeled `SeenAsFaulty`.
        let faulty_iter = era1_metadata
            .seen_as_faulty
            .iter()
            .map(|&public_key| (public_key.clone(), ValidatorChange::SeenAsFaulty));

        // Faulty peers in `era1` but not `era0` which are also validators in `era1` are labeled
        // `Banned`.
        let banned_iter = era1_metadata
            .faulty
            .difference(era0_metadata.faulty)
            .filter_map(|public_key| {
                if era1_metadata.validators.contains(public_key) {
                    Some((public_key.clone(), ValidatorChange::Banned))
                } else {
                    None
                }
            });

        // Peers which cannot propose in `era1` but can in `era0` and which are also validators in
        // `era1` are labeled `CannotPropose`.
        let cannot_propose_iter = era1_metadata
            .cannot_propose
            .difference(era0_metadata.cannot_propose)
            .filter_map(|public_key| {
                if era1_metadata.validators.contains(public_key) {
                    Some((public_key.clone(), ValidatorChange::CannotPropose))
                } else {
                    None
                }
            });

        ValidatorChanges(
            removed_iter
                .chain(faulty_iter)
                .chain(added_iter)
                .chain(banned_iter)
                .chain(cannot_propose_iter)
                .collect(),
        )
    }
}

#[derive(Clone)]
struct EraMetadata<'a> {
    validators: HashSet<&'a PublicKey>,
    seen_as_faulty: Vec<&'a PublicKey>,
    faulty: &'a HashSet<PublicKey>,
    cannot_propose: &'a HashSet<PublicKey>,
}

impl<'a> From<&'a Era> for EraMetadata<'a> {
    fn from(era: &'a Era) -> Self {
        let seen_as_faulty = era
            .consensus
            .validators_with_evidence()
            .into_iter()
            .collect();

        let validators = era.validators().keys().collect();
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
    use std::iter;

    use super::*;
    use crate::{crypto::AsymmetricKeyExt, testing::TestRng};

    fn preset_validators(rng: &mut TestRng) -> HashSet<PublicKey> {
        iter::repeat_with(|| PublicKey::random(rng))
            .take(5)
            .collect()
    }

    #[test]
    fn should_report_added() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era1_metadata = era0_metadata.clone();
        let added_validator = PublicKey::random(&mut rng);
        let expected_change = vec![(added_validator.clone(), ValidatorChange::Added)];
        era1_metadata.validators.insert(&added_validator);

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change.0);
    }

    #[test]
    fn should_report_removed() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let era1_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era0_metadata = era1_metadata.clone();
        let removed_validator = PublicKey::random(&mut rng);
        let expected_change = vec![(removed_validator.clone(), ValidatorChange::Removed)];
        era0_metadata.validators.insert(&removed_validator);

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change.0)
    }

    #[test]
    fn should_report_seen_as_faulty_in_new_era() {
        let mut rng = crate::new_rng();

        let seen_as_faulty_in_old_era = PublicKey::random(&mut rng);
        let era0_metadata = EraMetadata {
            validators: Default::default(),
            seen_as_faulty: vec![&seen_as_faulty_in_old_era],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };
        let seen_as_faulty_in_new_era = PublicKey::random(&mut rng);
        let era1_metadata = EraMetadata {
            validators: Default::default(),
            seen_as_faulty: vec![&seen_as_faulty_in_new_era],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let expected_change = vec![(
            seen_as_faulty_in_new_era.clone(),
            ValidatorChange::SeenAsFaulty,
        )];
        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change.0)
    }

    #[test]
    fn should_report_banned() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let faulty = validators.iter().next().unwrap();

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era1_metadata = era0_metadata.clone();
        let faulty_set = iter::once(faulty.clone()).collect();
        era1_metadata.faulty = &faulty_set;

        let expected_change = vec![(faulty.clone(), ValidatorChange::Banned)];
        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change.0)
    }

    #[test]
    fn should_not_report_banned_if_in_both_eras() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let faulty = validators.iter().next().unwrap();

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &iter::once(faulty.clone()).collect(),
            cannot_propose: &Default::default(),
        };
        let era1_metadata = era0_metadata.clone();

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert!(actual_change.0.is_empty());
    }

    #[test]
    fn should_not_report_banned_if_not_a_validator_in_new_era() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let faulty = PublicKey::random(&mut rng);

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era1_metadata = era0_metadata.clone();
        let faulty_set = iter::once(faulty).collect();
        era1_metadata.faulty = &faulty_set;

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert!(actual_change.0.is_empty());
    }

    #[test]
    fn should_report_cannot_propose() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let cannot_propose = validators.iter().next().unwrap();

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era1_metadata = era0_metadata.clone();
        let cannot_propose_set = iter::once(cannot_propose.clone()).collect();
        era1_metadata.cannot_propose = &cannot_propose_set;

        let expected_change = vec![(cannot_propose.clone(), ValidatorChange::CannotPropose)];
        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert_eq!(expected_change, actual_change.0)
    }

    #[test]
    fn should_not_report_cannot_propose_if_in_both_eras() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let cannot_propose = validators.iter().next().unwrap();

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &iter::once(cannot_propose.clone()).collect(),
        };
        let era1_metadata = era0_metadata.clone();

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert!(actual_change.0.is_empty());
    }

    #[test]
    fn should_not_report_cannot_propose_if_not_a_validator_in_new_era() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let cannot_propose = PublicKey::random(&mut rng);

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &Default::default(),
            cannot_propose: &Default::default(),
        };

        let mut era1_metadata = era0_metadata.clone();
        let cannot_propose_set = iter::once(cannot_propose).collect();
        era1_metadata.cannot_propose = &cannot_propose_set;

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert!(actual_change.0.is_empty());
    }

    #[test]
    fn should_report_no_status_change() {
        let mut rng = crate::new_rng();
        let validators = preset_validators(&mut rng);

        let era0_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: validators.iter().collect(),
            faulty: &validators,
            cannot_propose: &validators,
        };
        let era1_metadata = EraMetadata {
            validators: validators.iter().collect(),
            seen_as_faulty: vec![],
            faulty: &validators,
            cannot_propose: &validators,
        };

        let actual_change = ValidatorChanges::new_from_metadata(era0_metadata, era1_metadata);
        assert!(actual_change.0.is_empty());
    }
}
