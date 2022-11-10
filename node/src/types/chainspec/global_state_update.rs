use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    path::Path,
};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    file_utils, AsymmetricType, Key, PublicKey,
};

use super::error::GlobalStateUpdateLoadError;

const GLOBAL_STATE_UPDATE_FILENAME: &str = "global_state.toml";

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct GlobalStateUpdateEntry {
    key: String,
    value: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct GlobalStateUpdateConfig {
    // This must not be an optional field since it would be possible to accidentally provide TOML
    // where the provided validators were ignored, i.e. if `validators = [...]` was present _below_
    // the first `[[entries]]` value.
    validators: Vec<String>,
    entries: Vec<GlobalStateUpdateEntry>,
}

impl GlobalStateUpdateConfig {
    /// Returns `Self` and the raw bytes of the file.
    ///
    /// If the file doesn't exist, returns `Ok(None)`.
    pub(super) fn from_dir<P: AsRef<Path>>(
        path: P,
    ) -> Result<Option<(Self, Bytes)>, GlobalStateUpdateLoadError> {
        let update_path = path.as_ref().join(GLOBAL_STATE_UPDATE_FILENAME);
        if !update_path.is_file() {
            return Ok(None);
        }
        let bytes = file_utils::read_file(update_path)?;
        let config: GlobalStateUpdateConfig = toml::from_slice(&bytes)?;
        Ok(Some((config, Bytes::from(bytes))))
    }
}

/// Type storing the information about modifications to be applied to the global state.
///
/// It stores the serialized `StoredValue`s corresponding to keys to be modified, and for the case
/// where the validator set is being modified in any way, the full set of post-upgrade validators.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug)]
pub struct GlobalStateUpdate {
    pub(crate) validators: BTreeSet<PublicKey>,
    pub(crate) entries: BTreeMap<Key, Bytes>,
}

impl ToBytes for GlobalStateUpdate {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.validators.write_bytes(writer)?;
        self.entries.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.validators.serialized_length() + self.entries.serialized_length()
    }
}

impl FromBytes for GlobalStateUpdate {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (validators, remainder) = BTreeSet::<PublicKey>::from_bytes(bytes)?;
        let (entries, remainder) = BTreeMap::<Key, Bytes>::from_bytes(remainder)?;
        let global_state_update = GlobalStateUpdate {
            entries,
            validators,
        };
        Ok((global_state_update, remainder))
    }
}

impl TryFrom<GlobalStateUpdateConfig> for GlobalStateUpdate {
    type Error = GlobalStateUpdateLoadError;

    fn try_from(config: GlobalStateUpdateConfig) -> Result<Self, Self::Error> {
        let mut validators = BTreeSet::new();
        for (index, validator) in config.validators.into_iter().enumerate() {
            let public_key = PublicKey::from_hex(&validator).map_err(|error| {
                GlobalStateUpdateLoadError::DecodingKeyFromStr(format!(
                    "failed to decode validator public key {}: {}",
                    index, error
                ))
            })?;
            let _ = validators.insert(public_key);
        }

        let mut entries = BTreeMap::new();
        for (index, entry) in config.entries.into_iter().enumerate() {
            let key = Key::from_formatted_str(&entry.key).map_err(|error| {
                GlobalStateUpdateLoadError::DecodingKeyFromStr(format!(
                    "failed to decode entry key {}: {}",
                    index, error
                ))
            })?;
            let value = base64::decode(&entry.value)?.into();
            let _ = entries.insert(key, value);
        }

        Ok(GlobalStateUpdate {
            validators,
            entries,
        })
    }
}

#[cfg(test)]
impl GlobalStateUpdate {
    pub fn random(rng: &mut TestRng) -> Self {
        let mut validators = BTreeSet::new();
        if rng.gen() {
            let count = rng.gen_range(5..10);
            for _ in 0..count {
                validators.insert(PublicKey::random(rng));
            }
        }

        let count = rng.gen_range(0..10);
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            entries.insert(rng.gen(), rng.gen());
        }

        Self {
            validators,
            entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_state_update_bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let update = GlobalStateUpdate::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&update);
    }
}
