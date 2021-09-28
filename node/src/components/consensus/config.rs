use std::{path::Path, sync::Arc};

use datasize::DataSize;
use serde::Deserialize;

use casper_types::{ProtocolVersion, PublicKey, SecretKey};

use crate::{
    components::consensus::{protocols::highway::config::Config as HighwayConfig, EraId},
    crypto::hash::Digest,
    types::{chainspec::HighwayConfig as HighwayProtocolConfig, Chainspec, TimeDiff, Timestamp},
    utils::{External, LoadError, Loadable},
};

/// Consensus configuration.
#[derive(DataSize, Debug, Deserialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// Path to secret key file.
    pub(crate) secret_key_path: External,
    /// Highway-specific node configuration.
    pub(crate) highway: HighwayConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            secret_key_path: External::Missing,
            highway: HighwayConfig::default(),
        }
    }
}

impl Config {
    /// Loads the secret key from the configuration file and derives the public key.
    #[allow(clippy::type_complexity)]
    pub(crate) fn load_keys<P: AsRef<Path>>(
        &self,
        root: P,
    ) -> Result<(Arc<SecretKey>, PublicKey), LoadError<<Arc<SecretKey> as Loadable>::Error>> {
        let secret_signing_key: Arc<SecretKey> = self.secret_key_path.clone().load(root)?;
        let public_key: PublicKey = PublicKey::from(secret_signing_key.as_ref());
        Ok((secret_signing_key, public_key))
    }
}

pub trait ChainspecConsensusExt {
    /// Returns the ID of the last activation era, i.e. the era immediately after the most recent
    /// upgrade or restart.
    fn activation_era(&self) -> EraId;

    /// Returns the earliest era that is kept in memory. If the current era is N, that is usually N
    /// - 2, except that it's never at or before the most recent activation point.
    fn earliest_active_era(&self, current_era: EraId) -> EraId;

    /// Returns the earliest era whose switch block is needed to initialize the given era. For era
    /// N that will usually be N - A - 1, where A is the auction delay, except that switch block
    /// from before the most recent activation point are never used.
    fn earliest_switch_block_needed(&self, era_id: EraId) -> EraId;
}

impl ChainspecConsensusExt for Chainspec {
    fn activation_era(&self) -> EraId {
        self.protocol_config.activation_point.era_id()
    }

    fn earliest_active_era(&self, current_era: EraId) -> EraId {
        self.activation_era()
            .successor()
            .max(current_era.saturating_sub(2))
    }

    fn earliest_switch_block_needed(&self, era_id: EraId) -> EraId {
        self.activation_era().max(
            era_id
                .saturating_sub(1)
                .saturating_sub(self.core_config.auction_delay),
        )
    }
}

/// Consensus protocol configuration.
#[derive(DataSize, Debug)]
pub(crate) struct ProtocolConfig {
    pub(crate) highway_config: HighwayProtocolConfig,
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    pub(crate) unbonding_delay: u64,
    /// The network protocol version.
    #[data_size(skip)]
    pub(crate) protocol_version: ProtocolVersion,
    /// The first era ID after the last upgrade
    pub(crate) last_activation_point: EraId,
    /// Name of the network.
    pub(crate) name: String,
    /// Genesis timestamp, if available.
    pub(crate) genesis_timestamp: Option<Timestamp>,
    /// The chainspec hash: All nodes in the network agree on it, and it's unique to this network.
    pub(crate) chainspec_hash: Digest,
    /// The last emergency restart [`EraId`] (if there was one)
    pub(crate) last_emergency_restart: Option<EraId>,
}

impl From<&Chainspec> for ProtocolConfig {
    fn from(chainspec: &Chainspec) -> Self {
        ProtocolConfig {
            highway_config: chainspec.highway_config,
            era_duration: chainspec.core_config.era_duration,
            minimum_era_height: chainspec.core_config.minimum_era_height,
            auction_delay: chainspec.core_config.auction_delay,
            unbonding_delay: chainspec.core_config.unbonding_delay,
            protocol_version: chainspec.protocol_config.version,
            last_activation_point: chainspec.protocol_config.activation_point.era_id(),
            name: chainspec.network_config.name.clone(),
            genesis_timestamp: chainspec
                .protocol_config
                .activation_point
                .genesis_timestamp(),
            chainspec_hash: chainspec.hash(),
            last_emergency_restart: chainspec.protocol_config.last_emergency_restart,
        }
    }
}
