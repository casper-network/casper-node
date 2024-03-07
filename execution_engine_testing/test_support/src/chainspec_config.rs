use std::{
    convert::TryFrom,
    fs, io,
    path::{Path, PathBuf},
};

use log::error;
use once_cell::sync::Lazy;
use serde::Deserialize;

use casper_execution_engine::engine_state::{EngineConfig, EngineConfigBuilder};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    system::auction::VESTING_SCHEDULE_LENGTH_MILLIS, CoreConfig, FeeHandling, GenesisAccount,
    GenesisConfig, GenesisConfigBuilder, MintCosts, ProtocolVersion, RefundHandling, SystemConfig,
    TimeDiff, WasmConfig,
};

use crate::{
    DEFAULT_ACCOUNTS, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_MAX_QUERY_DEPTH,
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

/// This struct can be parsed from a TOML-encoded chainspec file.  It means that as the
/// chainspec format changes over versions, as long as we maintain the core config in this form
/// in the chainspec file, it can continue to be parsed as an `ChainspecConfig`.
#[derive(Deserialize, Clone, Default)]
pub struct ChainspecConfig {
    /// CoreConfig
    #[serde(rename = "core")]
    pub core_config: CoreConfig,
    /// WasmConfig.
    #[serde(rename = "wasm")]
    pub wasm_config: WasmConfig,
    /// SystemConfig
    #[serde(rename = "system_costs")]
    pub system_costs_config: SystemConfig,
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
            unbonding_delay,
            round_seigniorage_rate,
            ..
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

    /// Sets the vesting schedule period millis config option.
    pub fn with_max_associated_keys(&mut self, value: u32) -> &mut Self {
        self.core_config.max_associated_keys = value;
        self
    }

    /// Sets the vesting schedule period millis config option.
    pub fn with_vesting_schedule_period_millis(mut self, value: u64) -> Self {
        self.core_config.vesting_schedule_period = TimeDiff::from_millis(value);
        self
    }

    /// Sets the max delegators per validator config option.
    pub fn with_max_delegators_per_validator(mut self, value: u32) -> Self {
        self.core_config.max_delegators_per_validator = value;
        self
    }

    /// Sets the minimum delegation amount config option.
    pub fn with_minimum_delegation_amount(mut self, minimum_delegation_amount: u64) -> Self {
        self.core_config.minimum_delegation_amount = minimum_delegation_amount;
        self
    }

    /// Sets fee handling config option.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.core_config.fee_handling = fee_handling;
        self
    }

    /// Sets wasm config option.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = wasm_config;
        self
    }

    /// Sets mint costs.
    pub fn with_mint_costs(self, mint_costs: MintCosts) -> Self {
        self.system_costs_config.with_mint_costs(mint_costs);
        self
    }

    /// Sets wasm max stack height.
    pub fn with_wasm_max_stack_height(mut self, max_stack_height: u32) -> Self {
        self.wasm_config.max_stack_height = max_stack_height;
        self
    }

    /// Sets refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.core_config.refund_handling = refund_handling;
        self
    }

    /// Sets strict argument checking.
    pub fn with_strict_argument_checking(mut self, strict_argument_checking: bool) -> Self {
        self.core_config.strict_argument_checking = strict_argument_checking;
        self
    }

    /// Returns an engine config.
    pub fn engine_config(&self) -> EngineConfig {
        EngineConfigBuilder::new()
            .with_max_query_depth(DEFAULT_MAX_QUERY_DEPTH)
            .with_max_associated_keys(self.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(self.core_config.max_runtime_call_stack_height)
            .with_minimum_delegation_amount(self.core_config.minimum_delegation_amount)
            .with_strict_argument_checking(self.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(self.core_config.vesting_schedule_period.millis())
            .with_max_delegators_per_validator(self.core_config.max_delegators_per_validator)
            .with_wasm_config(self.wasm_config)
            .with_system_config(self.system_costs_config)
            .with_administrative_accounts(self.core_config.administrators.clone())
            .with_allow_auction_bids(self.core_config.allow_auction_bids)
            .with_allow_unrestricted_transfers(self.core_config.allow_unrestricted_transfers)
            .with_refund_handling(self.core_config.refund_handling)
            .with_fee_handling(self.core_config.fee_handling)
            .build()
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
