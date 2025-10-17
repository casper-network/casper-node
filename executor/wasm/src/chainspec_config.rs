use std::{
    convert::TryFrom,
    fs, io,
    path::{Path, PathBuf},
};

use log::error;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use serde::Deserialize;

use casper_execution_engine::engine_state::{
    engine_config::DEFAULT_ENABLE_ENTITY, EngineConfig, EngineConfigBuilder,
};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    system::auction::VESTING_SCHEDULE_LENGTH_MILLIS, ChainspecRegistry, CoreConfig, Digest,
    FeeHandling, GenesisAccount, GenesisConfig, HoldBalanceHandling, MintCosts, Motes,
    PricingHandling, ProtocolConfig, ProtocolVersion, PublicKey, RefundHandling, SecretKey,
    StorageCosts, SystemConfig, TimeDiff, WasmConfig,
};

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 1;
/// Default lock-in period is currently zero.
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 0;
/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 7;

/// Round seigniorage rate represented as a fraction of the total supply.
///
/// Annual issuance: 8%
/// Minimum round length: 2^14 ms
/// Ticks per year: 31536000000
///
/// (1+0.08)^((2^14)/31536000000)-1 is expressed as a fractional number below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(1, 4200000000000000000);
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// Default gas hold balance handling.
pub const DEFAULT_GAS_HOLD_BALANCE_HANDLING: HoldBalanceHandling = HoldBalanceHandling::Accrued;
/// Default gas hold interval in milliseconds.
pub const DEFAULT_GAS_HOLD_INTERVAL_MILLIS: u64 = 24 * 60 * 60 * 60;

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;

/// Default genesis config hash.
pub const DEFAULT_GENESIS_CONFIG_HASH: Digest = Digest::from_raw([42; 32]);

pub const DEFAULT_ACCOUNT_INITIAL_BALANCE: u64 = 10_000_000_000_000_000_000_u64;
/// Default proposer public key.
pub static DEFAULT_PROPOSER_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([198; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

pub(crate) static DEFAULT_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap());
pub(crate) static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ACCOUNT_SECRET_KEY));
pub static DEFAULT_ACCOUNT_HASH: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ACCOUNT_PUBLIC_KEY.to_account_hash());

/// Default accounts.
pub static DEFAULT_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut ret = Vec::new();
    let genesis_account = GenesisAccount::account(
        DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(genesis_account);
    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(proposer_account);
    let rng = &mut TestRng::new();
    for _ in 0..10 {
        let filler_account = GenesisAccount::account(
            PublicKey::random(rng),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            None,
        );
        ret.push(filler_account);
    }
    ret
});
/// Default [`ChainspecRegistry`].
pub static DEFAULT_CHAINSPEC_REGISTRY: Lazy<ChainspecRegistry> =
    Lazy::new(|| ChainspecRegistry::new_with_genesis(&[1, 2, 3], &[4, 5, 6]));

use casper_types::{account::AccountHash, testing::TestRng};

/// The name of the chainspec file on disk.
pub const CHAINSPEC_NAME: &str = "chainspec.toml";

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(CHAINSPEC_NAME)
});

/// A builder for an [`GenesisConfig`].
///
/// Any field that isn't specified will be defaulted.  See [the module docs](index.html) for the set
/// of default values.
#[derive(Default, Debug)]
pub struct GenesisConfigBuilder {
    accounts: Option<Vec<GenesisAccount>>,
    wasm_config: Option<WasmConfig>,
    system_config: Option<SystemConfig>,
    validator_slots: Option<u32>,
    auction_delay: Option<u64>,
    locked_funds_period_millis: Option<u64>,
    round_seigniorage_rate: Option<Ratio<u64>>,
    unbonding_delay: Option<u64>,
    genesis_timestamp_millis: Option<u64>,
    gas_hold_balance_handling: Option<HoldBalanceHandling>,
    gas_hold_interval_millis: Option<u64>,
    addressable_entity_enabled: Option<bool>,
    storage_costs: Option<StorageCosts>,
}

impl GenesisConfigBuilder {
    /// Creates a new `ExecConfig` builder.
    pub fn new() -> Self {
        GenesisConfigBuilder::default()
    }

    /// Sets the genesis accounts.
    pub fn with_accounts(mut self, accounts: Vec<GenesisAccount>) -> Self {
        self.accounts = Some(accounts);
        self
    }

    /// Sets the Wasm config options.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Sets the system config options.
    pub fn with_system_config(mut self, system_config: SystemConfig) -> Self {
        self.system_config = Some(system_config);
        self
    }

    /// Sets the validator slots config option.
    pub fn with_validator_slots(mut self, validator_slots: u32) -> Self {
        self.validator_slots = Some(validator_slots);
        self
    }

    /// Sets the auction delay config option.
    pub fn with_auction_delay(mut self, auction_delay: u64) -> Self {
        self.auction_delay = Some(auction_delay);
        self
    }

    /// Sets the locked funds period config option.
    pub fn with_locked_funds_period_millis(mut self, locked_funds_period_millis: u64) -> Self {
        self.locked_funds_period_millis = Some(locked_funds_period_millis);
        self
    }

    /// Sets the round seigniorage rate config option.
    pub fn with_round_seigniorage_rate(mut self, round_seigniorage_rate: Ratio<u64>) -> Self {
        self.round_seigniorage_rate = Some(round_seigniorage_rate);
        self
    }

    /// Sets the unbonding delay config option.
    pub fn with_unbonding_delay(mut self, unbonding_delay: u64) -> Self {
        self.unbonding_delay = Some(unbonding_delay);
        self
    }

    /// Sets the genesis timestamp config option.
    pub fn with_genesis_timestamp_millis(mut self, genesis_timestamp_millis: u64) -> Self {
        self.genesis_timestamp_millis = Some(genesis_timestamp_millis);
        self
    }

    /// Sets the enable addressable entity flag.
    pub fn with_addressable_entity_enabled(mut self, addressable_entity_enabled: bool) -> Self {
        self.addressable_entity_enabled = Some(addressable_entity_enabled);
        self
    }

    /// Sets the storage_costs handling.
    pub fn with_storage_costs(mut self, storage_costs: StorageCosts) -> Self {
        self.storage_costs = Some(storage_costs);
        self
    }

    /// Builds a new [`GenesisConfig`] object.
    pub fn build(self) -> GenesisConfig {
        GenesisConfig::new(
            self.accounts.unwrap_or_default(),
            self.wasm_config.unwrap_or_default(),
            self.system_config.unwrap_or_default(),
            self.validator_slots.unwrap_or(DEFAULT_VALIDATOR_SLOTS),
            self.auction_delay.unwrap_or(DEFAULT_AUCTION_DELAY),
            self.locked_funds_period_millis
                .unwrap_or(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS),
            self.round_seigniorage_rate
                .unwrap_or(DEFAULT_ROUND_SEIGNIORAGE_RATE),
            self.unbonding_delay.unwrap_or(DEFAULT_UNBONDING_DELAY),
            self.genesis_timestamp_millis
                .unwrap_or(DEFAULT_GENESIS_TIMESTAMP_MILLIS),
            self.gas_hold_balance_handling
                .unwrap_or(DEFAULT_GAS_HOLD_BALANCE_HANDLING),
            self.gas_hold_interval_millis
                .unwrap_or(DEFAULT_GAS_HOLD_INTERVAL_MILLIS),
            self.addressable_entity_enabled
                .unwrap_or(DEFAULT_ENABLE_ENTITY),
            self.storage_costs.unwrap_or_default(),
        )
    }
}

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
#[derive(Deserialize, Clone, Default, Debug)]
pub struct ChainspecConfig {
    /// Protocol config.
    #[serde(rename = "protocol")]
    pub protocol_config: ProtocolConfig,
    /// CoreConfig
    #[serde(rename = "core")]
    pub core_config: CoreConfig,
    /// WasmConfig.
    #[serde(rename = "wasm")]
    pub wasm_config: WasmConfig,
    /// SystemConfig
    #[serde(rename = "system_costs")]
    pub system_costs_config: SystemConfig,
    /// Storage costs.
    pub storage_costs: StorageCosts,
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

    /// Load from path.
    pub fn from_chainspec_path<P: AsRef<Path>>(filename: P) -> Result<Self, Error> {
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
        ChainspecConfig::from_path(filename)?
            .create_genesis_request(genesis_accounts, protocol_version)
    }

    /// Create genesis request from self.
    pub fn create_genesis_request(
        &self,
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<GenesisRequest, Error> {
        // if you get a compilation error here, make sure to update the builder below accordingly
        let ChainspecConfig {
            protocol_config: _protocol_config,
            core_config,
            wasm_config,
            system_costs_config,
            storage_costs,
        } = self;
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
            .with_wasm_config(*wasm_config)
            .with_system_config(*system_costs_config)
            .with_validator_slots(*validator_slots)
            .with_auction_delay(*auction_delay)
            .with_locked_funds_period_millis(locked_funds_period.millis())
            .with_round_seigniorage_rate(*round_seigniorage_rate)
            .with_unbonding_delay(*unbonding_delay)
            .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
            .with_storage_costs(*storage_costs)
            .build();

        Ok(GenesisRequest::new(
            DEFAULT_GENESIS_CONFIG_HASH,
            protocol_version,
            genesis_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        ))
    }

    /// Create a `RunGenesisRequest` using values from the local `chainspec.toml`.
    pub fn create_genesis_request_from_local_chainspec(
        genesis_accounts: Vec<GenesisAccount>,
        protocol_version: ProtocolVersion,
    ) -> Result<GenesisRequest, Error> {
        Self::create_genesis_request_from_chainspec(
            &*CHAINSPEC_SYMLINK,
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
        *self.wasm_config.v1_mut().max_stack_height_mut() = max_stack_height;
        self
    }

    /// Sets refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.core_config.refund_handling = refund_handling;
        self
    }

    /// Sets pricing handling config option.
    pub fn with_pricing_handling(mut self, pricing_handling: PricingHandling) -> Self {
        self.core_config.pricing_handling = pricing_handling;
        self
    }

    /// Sets strict argument checking.
    pub fn with_strict_argument_checking(mut self, strict_argument_checking: bool) -> Self {
        self.core_config.strict_argument_checking = strict_argument_checking;
        self
    }

    /// Sets the enable addressable entity flag.
    pub fn with_addressable_entity_enabled(mut self, addressable_entity_enabled: bool) -> Self {
        self.core_config.addressable_entity_enabled = addressable_entity_enabled;
        self
    }

    /// Returns the `max_associated_keys` setting from the core config.
    pub fn max_associated_keys(&self) -> u32 {
        self.core_config.max_associated_keys
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
            .with_enable_entity(self.core_config.addressable_entity_enabled)
            .with_storage_costs(self.storage_costs)
            .build()
    }
}

impl From<ChainspecConfig> for EngineConfig {
    fn from(chainspec_config: ChainspecConfig) -> Self {
        EngineConfigBuilder::new()
            .with_max_query_depth(DEFAULT_MAX_QUERY_DEPTH)
            .with_max_associated_keys(chainspec_config.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(
                chainspec_config.core_config.max_runtime_call_stack_height,
            )
            .with_minimum_delegation_amount(chainspec_config.core_config.minimum_delegation_amount)
            .with_strict_argument_checking(chainspec_config.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(
                chainspec_config
                    .core_config
                    .vesting_schedule_period
                    .millis(),
            )
            .with_max_delegators_per_validator(
                chainspec_config.core_config.max_delegators_per_validator,
            )
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .with_enable_entity(chainspec_config.core_config.addressable_entity_enabled)
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
            .with_storage_costs(chainspec_config.storage_costs)
            .with_addressable_entity_enabled(
                chainspec_config.core_config.addressable_entity_enabled,
            )
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
        let chainspec_config =
            ChainspecConfig::from_chainspec_path(path).expect("Expected chainspec to load");
        // Check that the loaded values matches values present in the local chainspec.
        assert_eq!(chainspec_config.core_config.auction_delay, 1);
    }

    #[test]
    fn should_get_exec_config_from_chainspec_values() {
        let path = &LOCAL_PATH.join(CHAINSPEC_NAME);
        let chainspec_config =
            ChainspecConfig::from_chainspec_path(path).expect("Expected chainspec to load");
        let config = GenesisConfig::try_from(chainspec_config).expect("Couln't build genesis");
        assert_eq!(config.auction_delay(), 1)
    }
}
