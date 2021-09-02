use std::{collections::BTreeMap, convert::TryFrom, path::Path};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    Key,
};

use super::error::GlobalStateUpdateLoadError;

use crate::utils::{self, Loadable};

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

impl Loadable for Option<GlobalStateUpdateConfig> {
    type Error = GlobalStateUpdateLoadError;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        let update_path = path.as_ref().join(GLOBAL_STATE_UPDATE_FILENAME);
        if !update_path.is_file() {
            return Ok(None);
        }
        let bytes = utils::read_file(update_path)?;
        let toml_update: GlobalStateUpdateConfig = toml::from_slice(&bytes)?;
        Ok(Some(toml_update))
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

    const RANDOM_BYTES_MAX_LENGTH: usize = 100;

    #[test]
    fn global_state_update_bytesrepr_roundtrip() {
        let update = {
            let mut rng = crate::new_rng();
            let mut map = BTreeMap::new();
            for _ in 0..rng.gen_range(0..10) {
                let len = rng.gen_range(0..RANDOM_BYTES_MAX_LENGTH);
                let mut vec_u8: Vec<u8> = Vec::with_capacity(len);
                for _ in 0..len {
                    vec_u8.push(rng.gen());
                }
                map.insert(rng.gen(), Bytes::from(vec_u8));
            }
            GlobalStateUpdate(map)
        };
        bytesrepr::test_serialization_roundtrip(&update);
    }
}
