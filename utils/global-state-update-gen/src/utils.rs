use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryInto,
};

use casper_execution_engine::shared::newtypes::Blake2bHash;
use casper_types::{system::auction::SeigniorageRecipientsSnapshot, PublicKey};

/// Parses a Blake2bHash from a string. Panics if parsing fails.
pub fn hash_from_str(hex_str: &str) -> Blake2bHash {
    (&base16::decode(hex_str).unwrap()[..]).try_into().unwrap()
}

pub struct ValidatorsDiff {
    pub added: BTreeSet<PublicKey>,
    pub removed: BTreeSet<PublicKey>,
}

/// Calculates the sets of added and removed validators between the two snapshots.
pub fn validators_diff(
    old_snapshot: &SeigniorageRecipientsSnapshot,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> ValidatorsDiff {
    let old_validators: BTreeSet<_> = old_snapshot
        .values()
        .flat_map(BTreeMap::keys)
        .cloned()
        .collect();
    let new_validators: BTreeSet<_> = new_snapshot
        .values()
        .flat_map(BTreeMap::keys)
        .cloned()
        .collect();

    ValidatorsDiff {
        added: new_validators
            .difference(&old_validators)
            .cloned()
            .collect(),
        removed: old_validators
            .difference(&new_validators)
            .cloned()
            .collect(),
    }
}
