use casper_types::PublicKey;

use super::era_supervisor::Era;

/// A change to a validator's status between two eras.
#[derive(Debug)]
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
        let mut changes = Vec::new();
        for pub_key in era0.validators().keys() {
            if !era1.validators().contains_key(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Removed));
            }
        }
        for pub_key in era1.consensus.validators_with_evidence() {
            changes.push((pub_key.clone(), ValidatorChange::SeenAsFaulty));
        }
        for pub_key in era1.validators().keys() {
            if !era0.validators().contains_key(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Added));
            }
            if era1.faulty.contains(pub_key) && !era0.faulty.contains(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::Banned));
            }
            if era1.cannot_propose.contains(pub_key) && !era0.cannot_propose.contains(pub_key) {
                changes.push((pub_key.clone(), ValidatorChange::CannotPropose));
            }
        }
        changes
    }
}
