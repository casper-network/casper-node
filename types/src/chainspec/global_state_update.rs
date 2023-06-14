#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, convert::TryFrom};
use thiserror::Error;

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    AsymmetricType, Key, PublicKey, U512,
};

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct GlobalStateUpdateEntry {
    key: String,
    value: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct GlobalStateUpdateValidatorInfo {
    public_key: String,
    weight: String,
}

/// Type storing global state update entries.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct GlobalStateUpdateConfig {
    validators: Option<Vec<GlobalStateUpdateValidatorInfo>>,
    entries: Vec<GlobalStateUpdateEntry>,
}

/// Type storing the information about modifications to be applied to the global state.
///
/// It stores the serialized `StoredValue`s corresponding to keys to be modified, and for the case
/// where the validator set is being modified in any way, the full set of post-upgrade validators.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct GlobalStateUpdate {
    /// Some with all validators (including pre-existent), if any change to the set is made.
    pub validators: Option<BTreeMap<PublicKey, U512>>,
    /// Global state key value pairs, which will be directly upserted into global state against
    /// the root hash of the final block of the era before the upgrade.
    pub entries: BTreeMap<Key, Bytes>,
}

impl GlobalStateUpdate {
    /// Returns a random `GlobalStateUpdate`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let mut validators = BTreeMap::new();
        if rng.gen() {
            let count = rng.gen_range(5..10);
            for _ in 0..count {
                validators.insert(PublicKey::random(rng), rng.gen::<U512>());
            }
        }

        let count = rng.gen_range(0..10);
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            entries.insert(rng.gen(), rng.gen());
        }

        Self {
            validators: Some(validators),
            entries,
        }
    }
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
        let (validators, remainder) = Option::<BTreeMap<PublicKey, U512>>::from_bytes(bytes)?;
        let (entries, remainder) = BTreeMap::<Key, Bytes>::from_bytes(remainder)?;
        let global_state_update = GlobalStateUpdate {
            entries,
            validators,
        };
        Ok((global_state_update, remainder))
    }
}

/// Error loading global state update file.
#[derive(Debug, Error)]
pub enum GlobalStateUpdateError {
    /// Error while decoding a key from a prefix formatted string.
    #[error("decoding key from formatted string error: {0}")]
    DecodingKeyFromStr(String),
    /// Error while decoding a key from a hex formatted string.
    #[error("decoding key from hex string error: {0}")]
    DecodingKeyFromHex(String),
    /// Error while decoding a public key weight from formatted string.
    #[error("decoding weight from decimal string error: {0}")]
    DecodingWeightFromStr(String),
    /// Error while decoding a serialized value from a base64 encoded string.
    #[error("decoding from base64 error: {0}")]
    DecodingFromBase64(#[from] base64::DecodeError),
}

impl TryFrom<GlobalStateUpdateConfig> for GlobalStateUpdate {
    type Error = GlobalStateUpdateError;

    fn try_from(config: GlobalStateUpdateConfig) -> Result<Self, Self::Error> {
        let mut validators: Option<BTreeMap<PublicKey, U512>> = None;
        if let Some(config_validators) = config.validators {
            let mut new_validators = BTreeMap::new();
            for (index, validator) in config_validators.into_iter().enumerate() {
                let public_key = PublicKey::from_hex(&validator.public_key).map_err(|error| {
                    GlobalStateUpdateError::DecodingKeyFromHex(format!(
                        "failed to decode validator public key {}: {:?}",
                        index, error
                    ))
                })?;
                let weight = U512::from_dec_str(&validator.weight).map_err(|error| {
                    GlobalStateUpdateError::DecodingWeightFromStr(format!(
                        "failed to decode validator weight {}: {}",
                        index, error
                    ))
                })?;
                let _ = new_validators.insert(public_key, weight);
            }
            validators = Some(new_validators);
        }

        let mut entries = BTreeMap::new();
        for (index, entry) in config.entries.into_iter().enumerate() {
            let key = Key::from_formatted_str(&entry.key).map_err(|error| {
                GlobalStateUpdateError::DecodingKeyFromStr(format!(
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
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn global_state_update_bytesrepr_roundtrip() {
        let mut rng = TestRng::from_entropy();
        let update = GlobalStateUpdate::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&update);
    }
}
