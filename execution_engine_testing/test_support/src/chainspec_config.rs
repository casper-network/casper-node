use std::{
    collections::BTreeSet,
    convert::TryFrom,
    fs, io,
    path::{Path, PathBuf},
};

use num_rational::Ratio;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::{
        engine_config::{FeeHandling, RefundHandling},
        genesis::ExecConfigBuilder,
        run_genesis_request::RunGenesisRequest,
        ExecConfig, GenesisAccount,
    },
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_types::{ProtocolVersion, PublicKey};

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
    FailedToCreateExecConfig,
    FailedToParseLockedFundsPeriod,
    FailedToCreateGenesisRequest,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum RefundHandlingConfig {
    Refund { refund_ratio: Ratio<u64> },
}

impl From<RefundHandlingConfig> for RefundHandling {
    fn from(value: RefundHandlingConfig) -> Self {
        match value {
            RefundHandlingConfig::Refund { refund_ratio } => {
                RefundHandling::Refund { refund_ratio }
            }
        }
    }
}

/// Fee handling config
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum FeeHandlingConfig {
    /// Transaction fees are paid to the block proposer.
    ///
    /// This is the default option for public chains.
    PayToProposer,
    /// Transaction fees are accumulated in a special rewards purse.
    ///
    /// This setting makes sense for some private chains.
    Accumulate,
}

impl From<FeeHandlingConfig> for FeeHandling {
    fn from(value: FeeHandlingConfig) -> Self {
        match value {
            FeeHandlingConfig::PayToProposer => FeeHandling::PayToProposer,
            FeeHandlingConfig::Accumulate => FeeHandling::Accumulate,
        }
    }
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
    pub(crate) locked_funds_period: String,
    /// The delay in number of eras for paying out the the unbonding amount.
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
    /// Auction entrypoints such as "add_bid" or "delegate" are disabled if this flag is set to
    /// `true`. Setting up this option makes sense only for private chains where validator set
    /// rotation is unnecessary.
    pub(crate) allow_auction_bids: bool,
    /// Allows unrestricted transfers between users.
    pub(crate) allow_unrestricted_transfers: bool,
    /// Administrative accounts are valid option for for a private chain only.
    #[serde(default)]
    pub(crate) administrators: BTreeSet<PublicKey>,
    // Refund handling.
    pub(crate) refund_handling: RefundHandlingConfig,
    // Fee handling.
    pub(crate) fee_handling: FeeHandlingConfig,
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

    pub(crate) fn create_genesis_request_from_chainspec<P: AsRef<Path>>(
        filename: P,
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<RunGenesisRequest, Error> {
        let path = filename.as_ref();
        let bytes = fs::read(path).map_err(|error| Error::FailedToLoadChainspec {
            path: path.into(),
            error,
        })?;
        let chainspec_config: ChainspecConfig =
            toml::from_slice(&bytes).map_err(Error::FailedToParseChainspec)?;
        let locked_funds_period_millis =
            humantime::parse_duration(&*chainspec_config.core_config.locked_funds_period)
                .map_err(|_| Error::FailedToCreateGenesisRequest)?
                .as_millis() as u64;
        let exec_config = ExecConfigBuilder::default()
            .with_accounts(genesis_accounts)
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .with_validator_slots(chainspec_config.core_config.validator_slots)
            .with_auction_delay(chainspec_config.core_config.auction_delay)
            .with_locked_funds_period_millis(locked_funds_period_millis)
            .with_round_seigniorage_rate(chainspec_config.core_config.round_seigniorage_rate)
            .with_unbonding_delay(chainspec_config.core_config.unbonding_delay)
            .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
            .with_refund_handling(chainspec_config.core_config.refund_handling.into())
            .with_fee_handling(chainspec_config.core_config.fee_handling.into())
            .build();
        Ok(RunGenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            protocol_version,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        ))
    }

    /// Create a `RunGenesisRequest` using values from the production `chainspec.toml`.
    pub fn create_genesis_request_from_production_chainspec(
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<RunGenesisRequest, Error> {
        Self::create_genesis_request_from_chainspec(
            &*PRODUCTION_PATH,
            genesis_accounts,
            protocol_version,
        )
    }
}

impl TryFrom<ChainspecConfig> for ExecConfig {
    type Error = Error;

    fn try_from(chainspec_config: ChainspecConfig) -> Result<Self, Self::Error> {
        let locked_funds_period_millis =
            humantime::parse_duration(&*chainspec_config.core_config.locked_funds_period)
                .map_err(|_| Error::FailedToCreateExecConfig)?
                .as_millis() as u64;
        Ok(ExecConfigBuilder::default()
            .with_accounts(DEFAULT_ACCOUNTS.clone())
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .with_validator_slots(chainspec_config.core_config.validator_slots)
            .with_auction_delay(chainspec_config.core_config.auction_delay)
            .with_locked_funds_period_millis(locked_funds_period_millis)
            .with_round_seigniorage_rate(chainspec_config.core_config.round_seigniorage_rate)
            .with_unbonding_delay(chainspec_config.core_config.unbonding_delay)
            .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
            .with_refund_handling(chainspec_config.core_config.refund_handling.into())
            .with_fee_handling(chainspec_config.core_config.fee_handling.into())
            .build())
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
