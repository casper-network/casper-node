#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Key, ProtocolVersion, StoredValue,
};

use crate::{ActivationPoint, GlobalStateUpdate};

/// Configuration values associated with the protocol.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ProtocolConfig {
    /// Protocol version.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    pub version: ProtocolVersion,
    /// Whether we need to clear latest blocks back to the switch block just before the activation
    /// point or not.
    pub hard_reset: bool,
    /// This protocol config applies starting at the era specified in the activation point.
    pub activation_point: ActivationPoint,
    /// Any arbitrary updates we might want to make to the global state at the start of the era
    /// specified in the activation point.
    pub global_state_update: Option<GlobalStateUpdate>,
}

impl ProtocolConfig {
    /// The mapping of [`Key`]s to [`StoredValue`]s we will use to update global storage in the
    /// event of an emergency update.
    pub(crate) fn get_update_mapping(
        &self,
    ) -> Result<BTreeMap<Key, StoredValue>, bytesrepr::Error> {
        let state_update = match &self.global_state_update {
            Some(GlobalStateUpdate { entries, .. }) => entries,
            None => return Ok(BTreeMap::default()),
        };
        let mut update_mapping = BTreeMap::new();
        for (key, stored_value_bytes) in state_update {
            let stored_value = bytesrepr::deserialize(stored_value_bytes.clone().into())?;
            update_mapping.insert(*key, stored_value);
        }
        Ok(update_mapping)
    }

    /// Returns a random `ProtocolConfig`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let protocol_version = ProtocolVersion::from_parts(
            rng.gen_range(0..10),
            rng.gen::<u8>() as u32,
            rng.gen::<u8>() as u32,
        );
        let activation_point = ActivationPoint::random(rng);

        ProtocolConfig {
            version: protocol_version,
            hard_reset: rng.gen(),
            activation_point,
            global_state_update: None,
        }
    }
}

impl ToBytes for ProtocolConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.version.to_string().to_bytes()?);
        buffer.extend(self.hard_reset.to_bytes()?);
        buffer.extend(self.activation_point.to_bytes()?);
        buffer.extend(self.global_state_update.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.version.to_string().serialized_length()
            + self.hard_reset.serialized_length()
            + self.activation_point.serialized_length()
            + self.global_state_update.serialized_length()
    }
}

impl FromBytes for ProtocolConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version_string, remainder) = String::from_bytes(bytes)?;
        let version = ProtocolVersion::from_str(&protocol_version_string)
            .map_err(|_| bytesrepr::Error::Formatting)?;
        let (hard_reset, remainder) = bool::from_bytes(remainder)?;
        let (activation_point, remainder) = ActivationPoint::from_bytes(remainder)?;
        let (global_state_update, remainder) = Option::<GlobalStateUpdate>::from_bytes(remainder)?;
        let protocol_config = ProtocolConfig {
            version,
            hard_reset,
            activation_point,
            global_state_update,
        };
        Ok((protocol_config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn activation_point_bytesrepr_roundtrip() {
        let mut rng = TestRng::from_entropy();
        let activation_point = ActivationPoint::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&activation_point);
    }

    #[test]
    fn protocol_config_bytesrepr_roundtrip() {
        let mut rng = TestRng::from_entropy();
        let config = ProtocolConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
