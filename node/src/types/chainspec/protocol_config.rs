// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::str::FromStr;

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::error;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, ProtocolVersion,
};

use super::{ActivationPoint, GlobalStateUpdate};
#[cfg(test)]
use crate::testing::TestRng;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug)]
pub struct ProtocolConfig {
    #[data_size(skip)]
    pub(crate) version: ProtocolVersion,
    /// Whether we need to clear latest blocks back to the switch block just before the activation
    /// point or not.
    pub(crate) hard_reset: bool,
    /// This protocol config applies starting at the era specified in the activation point.
    pub(crate) activation_point: ActivationPoint,
    /// Any arbitrary updates we might want to make to the global state at the start of the era
    /// specified in the activation point.
    pub(crate) global_state_update: Option<GlobalStateUpdate>,
    /// The era ID in which the last emergency restart happened.
    pub(crate) last_emergency_restart: Option<EraId>,
}

impl ProtocolConfig {
    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub(super) fn is_valid(&self) -> bool {
        // If this is not an emergency restart config, assert the `last_emergency_restart` is `None`
        // or less than `activation_point`.
        if self.global_state_update.is_none() {
            if let Some(last_emergency_restart) = self.last_emergency_restart {
                let activation_point = self.activation_point.era_id();
                if last_emergency_restart >= activation_point {
                    error!(
                        %activation_point,
                        %last_emergency_restart,
                        "[protocol.last_emergency_restart] must be lower than \
                        [protocol.activation_point] in the chainspec."
                    );
                    return false;
                };
            }
            return true;
        }

        // If this IS an emergency restart config, assert the `last_emergency_restart` is `Some` and
        // equal to `activation_point`.
        let last_emergency_restart = match self.last_emergency_restart {
            Some(era_id) => era_id,
            None => {
                error!(
                    "[protocol.last_emergency_restart] must exist in the chainspec since a global \
                    state update was provided, implying this upgrade is an emergency restart."
                );
                return false;
            }
        };
        let activation_point = self.activation_point.era_id();
        if activation_point != last_emergency_restart {
            error!(
                %activation_point,
                %last_emergency_restart,
                "[protocol.last_emergency_restart] must equal [protocol.activation_point] in the \
                chainspec since a global state update was provided, implying this upgrade is an \
                emergency restart."
            );
            return false;
        }

        true
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let protocol_version = ProtocolVersion::from_parts(
            rng.gen_range(0..10),
            rng.gen::<u8>() as u32,
            rng.gen::<u8>() as u32,
        );
        let activation_point = ActivationPoint::random(rng);
        let last_emergency_restart = rng.gen::<bool>().then(|| rng.gen());

        ProtocolConfig {
            version: protocol_version,
            hard_reset: rng.gen(),
            activation_point,
            global_state_update: None,
            last_emergency_restart,
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
        buffer.extend(self.last_emergency_restart.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.version.to_string().serialized_length()
            + self.hard_reset.serialized_length()
            + self.activation_point.serialized_length()
            + self.global_state_update.serialized_length()
            + self.last_emergency_restart.serialized_length()
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
        let (last_emergency_restart, remainder) = Option::<EraId>::from_bytes(remainder)?;
        let protocol_config = ProtocolConfig {
            version,
            hard_reset,
            activation_point,
            global_state_update,
            last_emergency_restart,
        };
        Ok((protocol_config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_point_bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let activation_point = ActivationPoint::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&activation_point);
    }

    #[test]
    fn protocol_config_bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let config = ProtocolConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn toml_roundtrip() {
        let mut rng = crate::new_rng();
        let config = ProtocolConfig::random(&mut rng);
        let encoded = toml::to_string_pretty(&config).unwrap();
        let decoded = toml::from_str(&encoded).unwrap();
        assert_eq!(config, decoded);
    }

    #[test]
    fn should_validate_if_not_emergency_restart() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        // If `global_state_update` is `None` then config is valid if `last_emergency_restart` is
        // also `None`.
        protocol_config.global_state_update = None;
        protocol_config.last_emergency_restart = None;
        assert!(protocol_config.is_valid());

        // If `global_state_update` is `None` then config is valid if `last_emergency_restart` is
        // less than `activation_point`.
        let activation_point = EraId::new(rng.gen_range(2..u64::MAX));
        protocol_config.activation_point = ActivationPoint::EraId(activation_point);
        protocol_config.last_emergency_restart = Some(activation_point - 1);
        assert!(protocol_config.is_valid());
    }

    #[test]
    fn should_fail_to_validate_if_no_last_emergency_restart() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        // If `global_state_update` is `Some`, this is an emergency restart, so
        // `last_emergency_restart` should be `Some`.
        protocol_config.global_state_update = Some(GlobalStateUpdate::random(&mut rng));
        protocol_config.last_emergency_restart = None;
        assert!(!protocol_config.is_valid());
    }

    #[test]
    fn should_fail_to_validate_if_emergency_restart_with_invalid_last() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        let activation_point = EraId::new(rng.gen_range(2..u64::MAX));

        // If `global_state_update` is `Some`, this is an emergency restart, so
        // `last_emergency_restart` should match the given activation point.
        protocol_config.global_state_update = Some(GlobalStateUpdate::random(&mut rng));
        protocol_config.activation_point = ActivationPoint::EraId(activation_point);

        protocol_config.last_emergency_restart = Some(activation_point + 1);
        assert!(!protocol_config.is_valid());

        protocol_config.last_emergency_restart = Some(activation_point - 1);
        assert!(!protocol_config.is_valid());
    }

    #[test]
    fn should_fail_to_validate_if_not_emergency_restart_with_invalid_last() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        let activation_point = EraId::new(rng.gen_range(2..u64::MAX));

        // If `global_state_update` is `None` then config is valid only if `last_emergency_restart`
        // is less than `activation_point`.
        protocol_config.global_state_update = None;
        protocol_config.activation_point = ActivationPoint::EraId(activation_point);
        protocol_config.last_emergency_restart = Some(activation_point);
        assert!(!protocol_config.is_valid());

        protocol_config.last_emergency_restart = Some(activation_point + 1);
        assert!(!protocol_config.is_valid());
    }
}
