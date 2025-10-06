//! Contains genesis configuration settings.

#[cfg(any(feature = "testing", test))]
use std::iter;

use num_rational::Ratio;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use crate::{
    AdministratorAccount, Chainspec, GenesisAccount, GenesisValidator, HoldBalanceHandling, Motes,
    PublicKey, SystemConfig, WasmConfig,
};

use super::StorageCosts;

/// Represents the details of a genesis process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisConfig {
    accounts: Vec<GenesisAccount>,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
    validator_slots: u32,
    auction_delay: u64,
    locked_funds_period_millis: u64,
    round_seigniorage_rate: Ratio<u64>,
    unbonding_delay: u64,
    genesis_timestamp_millis: u64,
    gas_hold_balance_handling: HoldBalanceHandling,
    gas_hold_interval_millis: u64,
    addressable_entity_enabled: bool,
    storage_costs: StorageCosts,
}

impl GenesisConfig {
    /// Creates a new genesis configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        accounts: Vec<GenesisAccount>,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
        validator_slots: u32,
        auction_delay: u64,
        locked_funds_period_millis: u64,
        round_seigniorage_rate: Ratio<u64>,
        unbonding_delay: u64,
        genesis_timestamp_millis: u64,
        gas_hold_balance_handling: HoldBalanceHandling,
        gas_hold_interval_millis: u64,
        addressable_entity_enabled: bool,
        storage_costs: StorageCosts,
    ) -> GenesisConfig {
        GenesisConfig {
            accounts,
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period_millis,
            round_seigniorage_rate,
            unbonding_delay,
            genesis_timestamp_millis,
            gas_hold_balance_handling,
            gas_hold_interval_millis,
            addressable_entity_enabled,
            storage_costs,
        }
    }

    /// Returns WASM config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }

    /// Returns all bonded genesis validators.
    pub fn get_bonded_validators(&self) -> impl Iterator<Item = &GenesisAccount> {
        self.accounts_iter()
            .filter(|&genesis_account| genesis_account.is_validator())
    }

    /// Returns all bonded genesis delegators.
    pub fn get_bonded_delegators(
        &self,
    ) -> impl Iterator<Item = (&PublicKey, &PublicKey, &Motes, &Motes)> {
        self.accounts
            .iter()
            .filter_map(|genesis_account| genesis_account.as_delegator())
    }

    /// Returns all genesis accounts.
    pub fn accounts(&self) -> &[GenesisAccount] {
        self.accounts.as_slice()
    }

    /// Returns an iterator over all genesis accounts.
    pub fn accounts_iter(&self) -> impl Iterator<Item = &GenesisAccount> {
        self.accounts.iter()
    }

    /// Returns an iterator over all administrative accounts.
    pub fn administrative_accounts(&self) -> impl Iterator<Item = &AdministratorAccount> {
        self.accounts
            .iter()
            .filter_map(GenesisAccount::as_administrator_account)
    }

    /// Adds new genesis account to the config.
    pub fn push_account(&mut self, account: GenesisAccount) {
        self.accounts.push(account)
    }

    /// Returns validator slots.
    pub fn validator_slots(&self) -> u32 {
        self.validator_slots
    }

    /// Returns auction delay.
    pub fn auction_delay(&self) -> u64 {
        self.auction_delay
    }

    /// Returns locked funds period expressed in milliseconds.
    pub fn locked_funds_period_millis(&self) -> u64 {
        self.locked_funds_period_millis
    }

    /// Returns round seigniorage rate.
    pub fn round_seigniorage_rate(&self) -> Ratio<u64> {
        self.round_seigniorage_rate
    }

    /// Returns unbonding delay in eras.
    pub fn unbonding_delay(&self) -> u64 {
        self.unbonding_delay
    }

    /// Returns genesis timestamp expressed in milliseconds.
    pub fn genesis_timestamp_millis(&self) -> u64 {
        self.genesis_timestamp_millis
    }

    /// Returns gas hold balance handling.
    pub fn gas_hold_balance_handling(&self) -> HoldBalanceHandling {
        self.gas_hold_balance_handling
    }

    /// Returns gas hold interval expressed in milliseconds.
    pub fn gas_hold_interval_millis(&self) -> u64 {
        self.gas_hold_interval_millis
    }

    /// Enable entity.
    pub fn enable_entity(&self) -> bool {
        self.addressable_entity_enabled
    }

    /// Set enable entity.
    pub fn set_enable_entity(&mut self, enable: bool) {
        self.addressable_entity_enabled = enable
    }

    /// Push genesis validator.
    pub fn push_genesis_validator(
        &mut self,
        public_key: &PublicKey,
        genesis_validator: GenesisValidator,
    ) {
        if let Some(genesis_account) = self
            .accounts
            .iter_mut()
            .find(|x| &x.public_key() == public_key)
        {
            genesis_account.try_set_validator(genesis_validator);
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<GenesisConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisConfig {
        let count = rng.gen_range(1..10);

        let accounts = iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        let wasm_config = rng.gen();

        let system_config = rng.gen();

        let validator_slots = rng.gen();

        let auction_delay = rng.gen();

        let locked_funds_period_millis = rng.gen();

        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1..1_000_000_000),
            rng.gen_range(1..1_000_000_000),
        );

        let unbonding_delay = rng.gen();

        let genesis_timestamp_millis = rng.gen();
        let gas_hold_balance_handling = rng.gen();
        let gas_hold_interval_millis = rng.gen();
        let storage_costs = rng.gen();

        GenesisConfig {
            accounts,
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period_millis,
            round_seigniorage_rate,
            unbonding_delay,
            genesis_timestamp_millis,
            gas_hold_balance_handling,
            gas_hold_interval_millis,
            addressable_entity_enabled: false,
            storage_costs,
        }
    }
}

impl From<&Chainspec> for GenesisConfig {
    fn from(chainspec: &Chainspec) -> Self {
        let genesis_timestamp_millis = chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
            .map_or(0, |timestamp| timestamp.millis());
        let gas_hold_interval_millis = chainspec.core_config.gas_hold_interval.millis();
        let gas_hold_balance_handling = chainspec.core_config.gas_hold_balance_handling;
        let storage_costs = chainspec.storage_costs;
        GenesisConfig {
            accounts: chainspec.network_config.accounts_config.clone().into(),
            wasm_config: chainspec.wasm_config,
            system_config: chainspec.system_costs_config,
            validator_slots: chainspec.core_config.validator_slots,
            auction_delay: chainspec.core_config.auction_delay,
            locked_funds_period_millis: chainspec.core_config.locked_funds_period.millis(),
            round_seigniorage_rate: chainspec.core_config.round_seigniorage_rate,
            unbonding_delay: chainspec.core_config.unbonding_delay,
            genesis_timestamp_millis,
            gas_hold_balance_handling,
            gas_hold_interval_millis,
            addressable_entity_enabled: chainspec.core_config.addressable_entity_enabled,
            storage_costs,
        }
    }
}
