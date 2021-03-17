use std::path::PathBuf;

use datasize::DataSize;
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_types::SecretKey;

use crate::{
    components::consensus::EraId,
    crypto::hash::Digest,
    types::{chainspec::HighwayConfig, Chainspec, TimeDiff, Timestamp},
    utils::External,
};

/// Consensus configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to secret key file.
    pub secret_key_path: External<SecretKey>,
    /// Path to the folder where unit hash files will be stored.
    pub unit_hashes_folder: PathBuf,
    /// The duration for which incoming vertices with missing dependencies are kept in a queue.
    pub pending_vertex_timeout: TimeDiff,
    /// The frequency at which we will ask peers for their latest state.
    pub request_latest_state_timeout: TimeDiff,
    /// The maximum number of blocks by which execution is allowed to lag behind finalization.
    /// If it is more than that, consensus will pause, and resume once the executor has caught up.
    pub max_execution_delay: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            secret_key_path: External::Missing,
            unit_hashes_folder: Default::default(),
            pending_vertex_timeout: "10sec".parse().unwrap(),
            request_latest_state_timeout: "5sec".parse().unwrap(),
            max_execution_delay: 3,
        }
    }
}

/// Consensus protocol configuration.
#[derive(DataSize, Debug)]
pub(crate) struct ProtocolConfig {
    pub(crate) highway_config: HighwayConfig,
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    pub(crate) unbonding_delay: u64,
    /// The network protocol version.
    #[data_size(skip)]
    pub(crate) protocol_version: Version,
    /// The first era ID after the last upgrade
    pub(crate) last_activation_point: EraId,
    /// Name of the network.
    pub(crate) name: String,
    /// Genesis timestamp, if available.
    pub(crate) genesis_timestamp: Option<Timestamp>,
    /// The chainspec hash: All nodes in the network agree on it, and it's unique to this network.
    pub(crate) chainspec_hash: Digest,
}

impl From<&Chainspec> for ProtocolConfig {
    fn from(chainspec: &Chainspec) -> Self {
        ProtocolConfig {
            highway_config: chainspec.highway_config,
            era_duration: chainspec.core_config.era_duration,
            minimum_era_height: chainspec.core_config.minimum_era_height,
            auction_delay: chainspec.core_config.auction_delay,
            unbonding_delay: chainspec.core_config.unbonding_delay,
            protocol_version: chainspec.protocol_config.version.clone(),
            last_activation_point: chainspec.protocol_config.activation_point.era_id(),
            name: chainspec.network_config.name.clone(),
            genesis_timestamp: chainspec
                .protocol_config
                .activation_point
                .genesis_timestamp(),
            chainspec_hash: chainspec.hash(),
        }
    }
}
