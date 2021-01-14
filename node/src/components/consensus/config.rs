use std::path::PathBuf;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::SecretKey;

use crate::{
    components::chainspec_loader::{HighwayConfig, UpgradePoint},
    types::Timestamp,
    utils::External,
    Chainspec,
};

use super::era_supervisor::EraId;

/// Consensus configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Default, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to secret key file.
    pub secret_key_path: External<SecretKey>,
    /// Path to the folder where unit hash files will be stored.
    pub unit_hashes_folder: PathBuf,
}

/// Consensus protocol configuration.
#[derive(DataSize, Debug)]
pub(crate) struct ProtocolConfig {
    pub(crate) highway_config: HighwayConfig,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    pub(crate) unbonding_delay: u64,
    /// Name of the network.
    pub(crate) name: String,
    /// Genesis timestamp.
    pub(crate) timestamp: Timestamp,
    pub(crate) upgrades: Vec<UpgradePoint>,
    pub(crate) initial_era_id: EraId,
    pub(crate) initial_block_height: u64,
}

impl From<&Chainspec> for ProtocolConfig {
    fn from(c: &Chainspec) -> Self {
        ProtocolConfig {
            highway_config: c.genesis.highway_config.clone(),
            auction_delay: c.genesis.auction_delay,
            unbonding_delay: c.genesis.unbonding_delay,
            name: c.genesis.name.clone(),
            timestamp: c.genesis.timestamp,
            upgrades: c.upgrades.clone(),
            initial_era_id: EraId(c.genesis.initial_era_id.unwrap_or(0)),
            initial_block_height: c.genesis.initial_block_height.unwrap_or(0),
        }
    }
}
