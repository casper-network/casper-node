use std::iter;

use num_rational::Ratio;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use crate::{
    AdministratorAccount, Chainspec, FeeHandling, GenesisAccount, Motes, PublicKey, RefundHandling,
    SystemConfig, WasmConfig,
};

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 1;
/// Default lock-in period is currently zero.
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 0;
/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 7;
/// Default round seigniorage rate represented as a fractional number.
///
/// Annual issuance: 2%
/// Minimum round exponent: 14
/// Ticks per year: 31536000000
///
/// (1+0.02)^((2^14)/31536000000)-1 is expressed as a fraction below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(7, 175070816);
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// Default fee handling.
pub const DEFAULT_FEE_HANDLING: FeeHandling = FeeHandling::PayToProposer;
/// Default gas cost refund ratio.
pub const DEFAULT_REFUND_HANDLING: RefundHandling = RefundHandling::Refund {
    refund_ratio: Ratio::new_raw(99, 100),
};

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
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
}

impl GenesisConfig {
    /// Creates a new genesis configuration.
    ///
    /// New code should use [`GenesisConfigBuilder`] instead as some config options will otherwise
    /// be defaulted.
    #[deprecated(
        since = "3.0.0",
        note = "prefer to use ExecConfigBuilder to construct an ExecConfig"
    )]
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
            refund_handling: DEFAULT_REFUND_HANDLING,
            fee_handling: DEFAULT_FEE_HANDLING,
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
}

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

        let refund_handling = RefundHandling::Refund {
            refund_ratio: Ratio::new_raw(rng.gen_range(0..=100), 100),
        };

        let fee_handling = if rng.gen() {
            FeeHandling::Accumulate
        } else {
            FeeHandling::PayToProposer
        };

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
            refund_handling,
            fee_handling,
        }
    }
}

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
    refund_handling: Option<RefundHandling>,
    fee_handling: Option<FeeHandling>,
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

    /// Sets the refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.refund_handling = Some(refund_handling);
        self
    }

    /// Sets the fee handling config option.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = Some(fee_handling);
        self
    }

    /// Builds a new [`GenesisConfig`] object.
    pub fn build(self) -> GenesisConfig {
        GenesisConfig {
            accounts: self.accounts.unwrap_or_default(),
            wasm_config: self.wasm_config.unwrap_or_default(),
            system_config: self.system_config.unwrap_or_default(),
            validator_slots: self.validator_slots.unwrap_or(DEFAULT_VALIDATOR_SLOTS),
            auction_delay: self.auction_delay.unwrap_or(DEFAULT_AUCTION_DELAY),
            locked_funds_period_millis: self
                .locked_funds_period_millis
                .unwrap_or(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS),
            round_seigniorage_rate: self
                .round_seigniorage_rate
                .unwrap_or(DEFAULT_ROUND_SEIGNIORAGE_RATE),
            unbonding_delay: self.unbonding_delay.unwrap_or(DEFAULT_UNBONDING_DELAY),
            genesis_timestamp_millis: self
                .genesis_timestamp_millis
                .unwrap_or(DEFAULT_GENESIS_TIMESTAMP_MILLIS),
            refund_handling: self.refund_handling.unwrap_or(DEFAULT_REFUND_HANDLING),
            fee_handling: self.fee_handling.unwrap_or(DEFAULT_FEE_HANDLING),
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

        GenesisConfigBuilder::default()
            .with_accounts(chainspec.network_config.accounts_config.clone().into())
            .with_wasm_config(chainspec.wasm_config)
            .with_system_config(chainspec.system_costs_config)
            .with_validator_slots(chainspec.core_config.validator_slots)
            .with_auction_delay(chainspec.core_config.auction_delay)
            .with_locked_funds_period_millis(chainspec.core_config.locked_funds_period.millis())
            .with_round_seigniorage_rate(chainspec.core_config.round_seigniorage_rate)
            .with_unbonding_delay(chainspec.core_config.unbonding_delay)
            .with_genesis_timestamp_millis(genesis_timestamp_millis)
            .with_refund_handling(chainspec.core_config.refund_handling)
            .with_fee_handling(chainspec.core_config.fee_handling)
            .build()
    }
}
