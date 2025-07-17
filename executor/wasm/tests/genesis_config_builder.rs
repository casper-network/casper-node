//! A builder for an [`GenesisConfig`].
use casper_execution_engine::engine_state::engine_config::DEFAULT_ENABLE_ENTITY;
use casper_types::{
    GenesisAccount, GenesisConfig, HoldBalanceHandling, StorageCosts, SystemConfig, WasmConfig,
};
use num_rational::Ratio;

use crate::{
    DEFAULT_AUCTION_DELAY, DEFAULT_GAS_HOLD_BALANCE_HANDLING, DEFAULT_GAS_HOLD_INTERVAL_MILLIS,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS,
};

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
    enable_addressable_entity: Option<bool>,
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
    pub fn with_enable_addressable_entity(mut self, enable_addressable_entity: bool) -> Self {
        self.enable_addressable_entity = Some(enable_addressable_entity);
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
            self.enable_addressable_entity
                .unwrap_or(DEFAULT_ENABLE_ENTITY),
            self.storage_costs.unwrap_or_default(),
        )
    }
}
