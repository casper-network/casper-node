use std::{
    convert::TryFrom,
    fs, io,
    path::{Path, PathBuf},
};

use log::error;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    system::auction::VESTING_SCHEDULE_LENGTH_MILLIS, FeeHandling, GenesisAccount, GenesisConfig,
    GenesisConfigBuilder, ProtocolVersion, RefundHandling, SystemConfig, TimeDiff, WasmConfig,
};

use crate::{
    DEFAULT_ACCOUNTS, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS,
};

/// The name of the chainspec file on disk.
pub const CHAINSPEC_NAME: &str = "chainspec.toml";

/// Path to the production chainspec used in the Casper mainnet.
pub static PRODUCTION_PATH: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources/")
        .join(CHAINSPEC_NAME)
});

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    FailedToLoadChainspec {
        /// Path that failed to be read.
        path: PathBuf,
        /// The underlying OS error.
        error: io::Error,
    },
    FailedToParseChainspec(toml::de::Error),
    Validation,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct CoreConfig {
    /// The number of validator slots in the auction.
    pub(crate) validator_slots: u32,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    /// The period after genesis during which a genesis validator's bid is locked.
    pub(crate) locked_funds_period: TimeDiff,
    /// The period in which genesis validator's bid is released over time
    pub(crate) vesting_schedule_period: TimeDiff,
    /// The delay in number of eras for paying out the unbonding amount.
    pub(crate) unbonding_delay: u64,
    /// Round seigniorage rate represented as a fractional number.
    pub(crate) round_seigniorage_rate: Ratio<u64>,
    /// Maximum number of associated keys for a single account.
    pub(crate) max_associated_keys: u32,
    /// Maximum height of contract runtime call stack.
    pub(crate) max_runtime_call_stack_height: u32,
    /// The minimum bound of motes that can be delegated to a validator.
    pub(crate) minimum_delegation_amount: u64,
    /// Enables strict arguments checking when calling a contract.
    pub(crate) strict_argument_checking: bool,
    /// The maximum amount of delegators per validator.
    pub(crate) max_delegators_per_validator: Option<u32>,
    /// Refund handling.
    pub(crate) refund_handling: RefundHandling,
    /// Fee handling.
    pub(crate) fee_handling: FeeHandling,
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
    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let chainspec_config: ChainspecConfig =
            toml::from_slice(bytes).map_err(Error::FailedToParseChainspec)?;

        if !chainspec_config.is_valid() {
            return Err(Error::Validation);
        }

        Ok(chainspec_config)
    }

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| Error::FailedToLoadChainspec {
            path: path.to_path_buf(),
            error,
        })?;
        ChainspecConfig::from_bytes(&bytes)
    }

    pub(crate) fn from_chainspec_path<P: AsRef<Path>>(filename: P) -> Result<Self, Error> {
        Self::from_path(filename)
    }

    fn is_valid(&self) -> bool {
        if self.core_config.vesting_schedule_period
            > TimeDiff::from_millis(VESTING_SCHEDULE_LENGTH_MILLIS)
        {
            error!(
                "vesting schedule period too long (actual {}; maximum {})",
                self.core_config.vesting_schedule_period.millis(),
                VESTING_SCHEDULE_LENGTH_MILLIS,
            );
            return false;
        }

        true
    }

    pub(crate) fn create_genesis_request_from_chainspec<P: AsRef<Path>>(
        filename: P,
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<GenesisRequest, Error> {
        let chainspec_config = ChainspecConfig::from_path(filename)?;

        // if you get a compilation error here, make sure to update the builder below accordingly
        let ChainspecConfig {
            core_config,
            wasm_config,
            system_costs_config,
        } = chainspec_config;
        let CoreConfig {
            validator_slots,
            auction_delay,
            locked_funds_period,
            vesting_schedule_period: _,
            unbonding_delay,
            round_seigniorage_rate,
            max_associated_keys: _,
            max_runtime_call_stack_height: _,
            minimum_delegation_amount: _,
            strict_argument_checking: _,
            max_delegators_per_validator: _,
            refund_handling: _,
            fee_handling: _,
        } = core_config;

        let genesis_config = GenesisConfigBuilder::new()
            .with_accounts(genesis_accounts)
            .with_wasm_config(wasm_config)
            .with_system_config(system_costs_config)
            .with_validator_slots(validator_slots)
            .with_auction_delay(auction_delay)
            .with_locked_funds_period_millis(locked_funds_period.millis())
            .with_round_seigniorage_rate(round_seigniorage_rate)
            .with_unbonding_delay(unbonding_delay)
            .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
            .build();

        Ok(GenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            protocol_version,
            genesis_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        ))
    }

    /// Create a `RunGenesisRequest` using values from the production `chainspec.toml`.
    pub fn create_genesis_request_from_production_chainspec(
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<GenesisRequest, Error> {
        Self::create_genesis_request_from_chainspec(
            &*PRODUCTION_PATH,
            genesis_accounts,
            protocol_version,
        )
    }
}

impl TryFrom<ChainspecConfig> for GenesisConfig {
    type Error = Error;

    fn try_from(chainspec_config: ChainspecConfig) -> Result<Self, Self::Error> {
        Ok(GenesisConfigBuilder::new()
            .with_accounts(DEFAULT_ACCOUNTS.clone())
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .with_validator_slots(chainspec_config.core_config.validator_slots)
            .with_auction_delay(chainspec_config.core_config.auction_delay)
            .with_locked_funds_period_millis(
                chainspec_config.core_config.locked_funds_period.millis(),
            )
            .with_round_seigniorage_rate(chainspec_config.core_config.round_seigniorage_rate)
            .with_unbonding_delay(chainspec_config.core_config.unbonding_delay)
            .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::TryFrom, path::PathBuf};

    use casper_types::GenesisConfig;
    use once_cell::sync::Lazy;

    use super::{ChainspecConfig, CHAINSPEC_NAME};

    pub static LOCAL_PATH: Lazy<PathBuf> =
        Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/local/"));

    #[test]
    fn should_load_chainspec_config_from_chainspec() {
        let path = &LOCAL_PATH.join(CHAINSPEC_NAME);
        let chainspec_config = ChainspecConfig::from_chainspec_path(path).unwrap();
        // Check that the loaded values matches values present in the local chainspec.
        assert_eq!(chainspec_config.core_config.auction_delay, 1);
    }

    #[test]
    fn should_get_exec_config_from_chainspec_values() {
        let path = &LOCAL_PATH.join(CHAINSPEC_NAME);
        let chainspec_config = ChainspecConfig::from_chainspec_path(path).unwrap();
        let config = GenesisConfig::try_from(chainspec_config).unwrap();
        assert_eq!(config.auction_delay(), 1)
    }
}
