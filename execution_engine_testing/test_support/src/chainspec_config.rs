use std::{
    convert::TryFrom,
    fs, io,
    path::{Path, PathBuf},
};

use num_rational::Ratio;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::ExecConfig,
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};

use crate::{
    chainspec_config::Error::CouldNotParseLockedFundsPeriod, DEFAULT_ACCOUNTS,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS,
};

/// The name of the chainspec file on disk.
pub const CHAINSPEC_NAME: &str = "chainspec.toml";

/// Path to the production chainspec used in the Casper mainnet.
pub static PRODUCTION_PATH: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/production/")
        .join(CHAINSPEC_NAME)
});

#[derive(Debug)]
pub enum Error {
    FailedToLoadChainspec {
        /// Path that failed to be read.
        path: PathBuf,
        /// The underlying OS error.
        error: io::Error,
    },
    FailedToParseChainspec(toml::de::Error),
    CouldNotCreateExecConfig,
    CouldNotParseLockedFundsPeriod,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct CoreConfig {
    pub(crate) era_duration: String,
    pub(crate) minimum_era_height: u64,
    pub(crate) validator_slots: u32,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    /// The period after genesis during which a genesis validator's bid is locked.
    pub(crate) locked_funds_period: String,
    /// The delay in number of eras for paying out the the unbonding amount.
    pub(crate) unbonding_delay: u64,
    /// Round seigniorage rate represented as a fractional number.
    pub(crate) round_seigniorage_rate: Ratio<u64>,
    /// Maximum number of associated keys for a single account.
    pub(crate) max_associated_keys: u32,
}

/// This struct can be parsed from a TOML-encoded chainspec file.  It means that as the
/// chainspec format changes over versions, as long as we maintain the core config in this form
/// in the chainspec file, it can continue to be parsed as an `ChainspecConfig`.
#[derive(Deserialize, Clone)]
pub struct ChainspecConfig {
    #[serde(rename = "core")]
    pub(crate) core_config: CoreConfig,
    #[serde(rename = "wasm")]
    pub(crate) wasm_config: WasmConfig,
    #[serde(rename = "system_costs")]
    pub(crate) system_costs_config: SystemConfig,
}

impl ChainspecConfig {
    pub(crate) fn from_chainspec_path<P: AsRef<Path>>(filename: P) -> Result<Self, Error> {
        let path = filename.as_ref();
        let bytes = fs::read(path).map_err(|error| Error::FailedToLoadChainspec {
            path: path.into(),
            error,
        })?;
        let chainspec_config: ChainspecConfig =
            toml::from_slice(&bytes).map_err(Error::FailedToParseChainspec)?;
        Ok(chainspec_config)
    }

    pub(crate) fn locked_funds_period(&self) -> Result<u64, Error> {
        let locked_funds_period_millis =
            humantime::parse_duration(&self.core_config.locked_funds_period)
                .map_err(|_| CouldNotParseLockedFundsPeriod)?
                .as_millis() as u64;
        Ok(locked_funds_period_millis)
    }
}

impl TryFrom<ChainspecConfig> for ExecConfig {
    type Error = Error;

    fn try_from(chainspec_config: ChainspecConfig) -> Result<Self, Self::Error> {
        let locked_funds_period_millis =
            humantime::parse_duration(&*chainspec_config.core_config.locked_funds_period)
                .map_err(|_| Error::CouldNotCreateExecConfig)?
                .as_millis() as u64;
        Ok(ExecConfig::new(
            DEFAULT_ACCOUNTS.clone(),
            chainspec_config.wasm_config,
            chainspec_config.system_costs_config,
            chainspec_config.core_config.validator_slots,
            chainspec_config.core_config.auction_delay,
            locked_funds_period_millis,
            chainspec_config.core_config.round_seigniorage_rate,
            chainspec_config.core_config.unbonding_delay,
            DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::TryFrom, path::PathBuf};

    use once_cell::sync::Lazy;

    use super::{ChainspecConfig, ExecConfig, CHAINSPEC_NAME};

    pub static LOCAL_PATH: Lazy<PathBuf> =
        Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/local/"));

    #[test]
    fn should_load_chainspec_config_from_chainspec() {
        let path = &LOCAL_PATH.join(CHAINSPEC_NAME);
        let chainspec_config = ChainspecConfig::from_chainspec_path(path).unwrap();
        // Check that the loaded values matches values present in the local chainspec.
        assert_eq!(chainspec_config.core_config.auction_delay, 3);
    }

    #[test]
    fn should_get_exec_config_from_chainspec_values() {
        let path = &LOCAL_PATH.join(CHAINSPEC_NAME);
        let chainspec_config = ChainspecConfig::from_chainspec_path(path).unwrap();
        let exec_config = ExecConfig::try_from(chainspec_config).unwrap();
        assert_eq!(exec_config.auction_delay(), 3)
    }
}
