use std::{collections::BTreeMap, convert::TryFrom, path::Path};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    file_utils, Key,
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
/// It stores the serialized `StoredValue`s corresponding to keys to be modified.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug)]
pub struct GlobalStateUpdate(pub(crate) BTreeMap<Key, Bytes>);

impl ToBytes for GlobalStateUpdate {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

#[cfg(test)]
impl GlobalStateUpdate {
    pub fn random(rng: &mut TestRng) -> Self {
        let entries = rng.gen_range(0..10);
        let mut map = BTreeMap::new();
        for _ in 0..entries {
            map.insert(rng.gen(), rng.gen());
        }
        Self(map)
    }
}

impl FromBytes for GlobalStateUpdate {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (update, remainder) = BTreeMap::<Key, Bytes>::from_bytes(bytes)?;
        let global_state_update = GlobalStateUpdate(update);
        Ok((global_state_update, remainder))
    }
}

impl TryFrom<GlobalStateUpdateConfig> for GlobalStateUpdate {
    type Error = GlobalStateUpdateLoadError;

    fn try_from(config: GlobalStateUpdateConfig) -> Result<Self, Self::Error> {
        let mut map = BTreeMap::new();
        for entry in config.entries.into_iter() {
            let key = Key::from_formatted_str(&entry.key).map_err(|err| {
                GlobalStateUpdateLoadError::DecodingKeyFromStr(format!("{}", err))
            })?;
            let value = base64::decode(&entry.value)?.into();
            let _ = map.insert(key, value);
        }
        Ok(GlobalStateUpdate(map))
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
