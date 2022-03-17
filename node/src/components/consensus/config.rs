use std::{path::Path, sync::Arc};

use datasize::DataSize;
use serde::Deserialize;

use casper_types::{PublicKey, SecretKey};

use crate::{
    components::consensus::{
        era_supervisor::PAST_OPEN_ERAS, protocols::highway::config::Config as HighwayConfig, EraId,
    },
    types::Chainspec,
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
    fn earliest_open_era(&self, current_era: EraId) -> EraId;

    /// Returns the earliest era whose switch block is needed to initialize the given era. For era
    /// N that will usually be N - A - 1, where A is the auction delay, except that switch block
    /// from before the most recent activation point are never used.
    fn earliest_switch_block_needed(&self, era_id: EraId) -> EraId;
}

impl ChainspecConsensusExt for Chainspec {
    fn activation_era(&self) -> EraId {
        self.protocol_config.activation_point.era_id()
    }

    fn earliest_open_era(&self, current_era: EraId) -> EraId {
        self.activation_era()
            .successor()
            .max(current_era.saturating_sub(PAST_OPEN_ERAS))
    }

    fn earliest_switch_block_needed(&self, era_id: EraId) -> EraId {
        self.activation_era().max(
            era_id
                .saturating_sub(1)
                .saturating_sub(self.core_config.auction_delay),
        )
    }
}
