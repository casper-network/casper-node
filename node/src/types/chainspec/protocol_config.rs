// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{collections::BTreeMap, str::FromStr};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::error;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, Key, ProtocolVersion, StoredValue,
};

use super::{ActivationPoint, GlobalStateUpdate};
use crate::types::BlockHeader;

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
    /// The era ID starting at which the new Merkle tree-based hashing scheme is applied.
    pub(crate) verifiable_chunked_hash_activation: EraId,
}

impl ProtocolConfig {
    /// The the mapping of [`Key`]s to [`StoredValue`]s we will use to update global storage in the
    /// event of an emergency update.
    pub(crate) fn get_update_mapping(
        &self,
    ) -> Result<BTreeMap<Key, StoredValue>, bytesrepr::Error> {
        let state_update = match &self.global_state_update {
            Some(GlobalStateUpdate(state_update)) => state_update,
            None => return Ok(BTreeMap::default()),
        };
        let mut update_mapping = BTreeMap::new();
        for (key, stored_value_bytes) in state_update {
            let stored_value = bytesrepr::deserialize(stored_value_bytes.clone().into())?;
            update_mapping.insert(*key, stored_value);
        }
        Ok(update_mapping)
    }

    /// Returns whether the block header belongs to the last block before the upgrade to the
    /// current protocol version.
    pub(crate) fn is_last_block_before_activation(&self, block_header: &BlockHeader) -> bool {
        block_header.protocol_version() < self.version
            && block_header.is_switch_block()
            && ActivationPoint::EraId(block_header.next_block_era_id()) == self.activation_point
    }

    /// Checks whether the values set in the config make sense and returns `false` if they don't.
    pub(super) fn is_valid(&self) -> bool {
        // Assert the `last_emergency_restart` is `None` or no more than `activation_point`.
        if let Some(last_emergency_restart) = self.last_emergency_restart {
            let activation_point = self.activation_point.era_id();
            if last_emergency_restart > activation_point {
                error!(
                    %activation_point,
                    %last_emergency_restart,
                    "[protocol.last_emergency_restart] cannot be greater than \
                    [protocol.activation_point] in the chainspec."
                );
                return false;
            };
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
        let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..5));

        ProtocolConfig {
            version: protocol_version,
            hard_reset: rng.gen(),
            activation_point,
            global_state_update: None,
            last_emergency_restart,
            verifiable_chunked_hash_activation,
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
        buffer.extend(self.verifiable_chunked_hash_activation.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.version.to_string().serialized_length()
            + self.hard_reset.serialized_length()
            + self.activation_point.serialized_length()
            + self.global_state_update.serialized_length()
            + self.last_emergency_restart.serialized_length()
            + self.verifiable_chunked_hash_activation.serialized_length()
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
        let (verifiable_chunked_hash_activation, remainder) = EraId::from_bytes(remainder)?;
        let protocol_config = ProtocolConfig {
            version,
            hard_reset,
            activation_point,
            global_state_update,
            last_emergency_restart,
            verifiable_chunked_hash_activation,
        };
        Ok((protocol_config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use crate::types::Block;

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
    fn should_perform_checks_without_global_state_update() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        let activation_point = EraId::new(rng.gen_range(2..u64::MAX));

        // We force `global_state_update` to be `None`.
        protocol_config.global_state_update = None;

        // The config is valid only if `last_emergency_restart` is no more than `activation_point`.
        protocol_config.activation_point = ActivationPoint::EraId(activation_point);
        protocol_config.last_emergency_restart = Some(activation_point);
        assert!(protocol_config.is_valid());

        protocol_config.last_emergency_restart = Some(activation_point + 1);
        assert!(!protocol_config.is_valid());
    }

    #[test]
    fn should_perform_checks_with_global_state_update() {
        let mut rng = crate::new_rng();
        let mut protocol_config = ProtocolConfig::random(&mut rng);

        let activation_point = EraId::new(rng.gen_range(2..u64::MAX));

        // We force `global_state_update` to be `Some`.
        protocol_config.global_state_update = Some(GlobalStateUpdate::random(&mut rng));

        // The config is valid only if `last_emergency_restart` is no more than `activation_point`.
        protocol_config.activation_point = ActivationPoint::EraId(activation_point);
        protocol_config.last_emergency_restart = Some(activation_point);
        assert!(protocol_config.is_valid());

        protocol_config.last_emergency_restart = Some(activation_point + 1);
        assert!(!protocol_config.is_valid());
    }

    #[test]
    fn should_recognize_blocks_before_activation_point() {
        let past_version = ProtocolVersion::from_parts(1, 0, 0);
        let current_version = ProtocolVersion::from_parts(2, 0, 0);
        let future_version = ProtocolVersion::from_parts(3, 0, 0);

        let upgrade_era = EraId::from(5);
        let previous_era = upgrade_era.saturating_sub(1);

        let verifiable_chunked_hash_activation = EraId::from(0);

        let mut rng = crate::new_rng();
        let protocol_config = ProtocolConfig {
            version: current_version,
            hard_reset: false,
            activation_point: ActivationPoint::EraId(upgrade_era),
            global_state_update: None,
            last_emergency_restart: None,
            verifiable_chunked_hash_activation,
        };

        // The block before this protocol version: a switch block with previous era and version.
        let block = Block::random_with_specifics(
            &mut rng,
            previous_era,
            100,
            past_version,
            true,
            verifiable_chunked_hash_activation,
            None,
        );
        assert!(protocol_config.is_last_block_before_activation(block.header()));

        // Not the activation point: wrong era.
        let block = Block::random_with_specifics(
            &mut rng,
            upgrade_era,
            100,
            past_version,
            true,
            verifiable_chunked_hash_activation,
            None,
        );
        assert!(!protocol_config.is_last_block_before_activation(block.header()));

        // Not the activation point: wrong version.
        let block = Block::random_with_specifics(
            &mut rng,
            previous_era,
            100,
            current_version,
            true,
            verifiable_chunked_hash_activation,
            None,
        );
        assert!(!protocol_config.is_last_block_before_activation(block.header()));
        let block = Block::random_with_specifics(
            &mut rng,
            previous_era,
            100,
            future_version,
            true,
            verifiable_chunked_hash_activation,
            None,
        );
        assert!(!protocol_config.is_last_block_before_activation(block.header()));

        // Not the activation point: not a switch block.
        let block = Block::random_with_specifics(
            &mut rng,
            previous_era,
            100,
            past_version,
            false,
            verifiable_chunked_hash_activation,
            None,
        );
        assert!(!protocol_config.is_last_block_before_activation(block.header()));
    }
}
