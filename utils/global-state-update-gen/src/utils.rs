use clap::ArgMatches;
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryInto,
};

use casper_types::{
    bytesrepr::ToBytes, checksummed_hex, system::auction::SeigniorageRecipientsSnapshotV2,
    AsymmetricType, Digest, Key, ProtocolVersion, PublicKey, StoredValue, U512,
};

/// Parses a Digest from a string. Panics if parsing fails.
pub fn hash_from_str(hex_str: &str) -> Digest {
    (&checksummed_hex::decode(hex_str).unwrap()[..])
        .try_into()
        .unwrap()
}

pub fn num_from_str(str: Option<&str>) -> Option<u32> {
    match str {
        Some(val) => val.parse::<u32>().ok(),
        None => None,
    }
}

pub fn protocol_version_from_matches(matches: &ArgMatches<'_>) -> ProtocolVersion {
    let major = num_from_str(matches.value_of("major")).unwrap_or(2);
    let minor = num_from_str(matches.value_of("minor")).unwrap_or(0);
    let patch = num_from_str(matches.value_of("patch")).unwrap_or(0);
    ProtocolVersion::from_parts(major, minor, patch)
}

pub(crate) fn print_validators(validators: &[ValidatorInfo]) {
    for validator in validators {
        println!("[[validators]]");
        println!("public_key = \"{}\"", validator.public_key.to_hex());
        println!("weight = \"{}\"", validator.weight);
        println!();
    }
    println!();
}

/// Prints a global state update entry in a format ready for inclusion in a TOML file.
pub(crate) fn print_entry(key: &Key, value: &StoredValue) {
    println!("[[entries]]");
    println!("key = \"{}\"", key.to_formatted_string());
    println!("value = \"{}\"", base64::encode(value.to_bytes().unwrap()));
    println!();
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) struct ValidatorInfo {
    pub public_key: PublicKey,
    pub weight: U512,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ValidatorsDiff {
    pub added: BTreeSet<PublicKey>,
    pub removed: BTreeSet<PublicKey>,
}

/// Calculates the sets of added and removed validators between the two snapshots.
pub fn validators_diff(
    old_snapshot: &SeigniorageRecipientsSnapshotV2,
    new_snapshot: &SeigniorageRecipientsSnapshotV2,
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
