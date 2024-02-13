//! Support for runtime configuration of the execution engine - as an integral property of the
//! `EngineState` instance.

use std::collections::BTreeSet;

use num_rational::Ratio;
use num_traits::One;

use casper_types::{
    account::AccountHash, FeeHandling, PublicKey, RefundHandling, SystemConfig, WasmConfig,
    DEFAULT_REFUND_HANDLING,
};

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;
/// Default max serialized size of `StoredValue`s.
#[deprecated(
    since = "3.2.0",
    note = "not used in `casper-execution-engine` config anymore"
)]
pub const DEFAULT_MAX_STORED_VALUE_SIZE: u32 = 8 * 1024 * 1024;
/// Default value for minimum delegation amount in motes.
pub const DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64 = 500 * 1_000_000_000;
/// Default value for strict argument checking.
pub const DEFAULT_STRICT_ARGUMENT_CHECKING: bool = false;
/// 91 days / 7 days in a week = 13 weeks
/// Length of total vesting schedule in days.
const VESTING_SCHEDULE_LENGTH_DAYS: usize = 91;
const DAY_MILLIS: usize = 24 * 60 * 60 * 1000;
/// Default length of total vesting schedule period expressed in days.
pub const DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS: u64 =
    VESTING_SCHEDULE_LENGTH_DAYS as u64 * DAY_MILLIS as u64;
/// Default value for allowing auction bids.
pub const DEFAULT_ALLOW_AUCTION_BIDS: bool = true;
/// Default value for allowing unrestricted transfers.
pub const DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS: bool = true;

/// Default fee handling.
pub const DEFAULT_FEE_HANDLING: FeeHandling = FeeHandling::PayToProposer;
/// Default compute rewards.
pub const DEFAULT_COMPUTE_REWARDS: bool = true;

/// The runtime configuration of the execution engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Max query depth of the engine.
    pub(crate) max_query_depth: u64,
    /// Maximum number of associated keys (i.e. map of
    /// [`AccountHash`](casper_types::account::AccountHash)s to
    /// [`Weight`](casper_types::account::Weight)s) for a single account.
    max_associated_keys: u32,
    max_runtime_call_stack_height: u32,
    minimum_delegation_amount: u64,
    /// This flag indicates if arguments passed to contracts are checked against the defined types.
    strict_argument_checking: bool,
    /// Vesting schedule period in milliseconds.
    vesting_schedule_period_millis: u64,
    max_delegators_per_validator: Option<u32>,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
    /// A private network specifies a list of administrative accounts.
    pub(crate) administrative_accounts: BTreeSet<AccountHash>,
    /// Auction entrypoints such as "add_bid" or "delegate" are disabled if this flag is set to
    /// `false`.
    pub(crate) allow_auction_bids: bool,
    /// Allow unrestricted transfers between normal accounts.
    ///
    /// If set to `true` accounts can transfer tokens between themselves without restrictions. If
    /// set to `false` tokens can be transferred only from normal accounts to administrators
    /// and administrators to normal accounts but not normal accounts to normal accounts.
    pub(crate) allow_unrestricted_transfers: bool,
    /// Refund handling config.
    pub(crate) refund_handling: RefundHandling,
    /// Fee handling.
    pub(crate) fee_handling: FeeHandling,
    /// Compute auction rewards.
    pub(crate) compute_rewards: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
            max_associated_keys: DEFAULT_MAX_ASSOCIATED_KEYS,
            max_runtime_call_stack_height: DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
            minimum_delegation_amount: DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            strict_argument_checking: DEFAULT_STRICT_ARGUMENT_CHECKING,
            vesting_schedule_period_millis: DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
            max_delegators_per_validator: None,
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
            administrative_accounts: Default::default(),
            allow_auction_bids: DEFAULT_ALLOW_AUCTION_BIDS,
            allow_unrestricted_transfers: DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS,
            refund_handling: DEFAULT_REFUND_HANDLING,
            fee_handling: DEFAULT_FEE_HANDLING,
            compute_rewards: DEFAULT_COMPUTE_REWARDS,
        }
    }
}

impl EngineConfig {
    /// Creates a new `EngineConfig` instance.
    ///
    /// New code should use [`EngineConfigBuilder`] instead as some config options will otherwise be
    /// defaulted.
    #[deprecated(
        since = "3.0.0",
        note = "prefer to use EngineConfigBuilder to construct an EngineConfig"
    )]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_query_depth: u64,
        max_associated_keys: u32,
        max_runtime_call_stack_height: u32,
        minimum_delegation_amount: u64,
        strict_argument_checking: bool,
        vesting_schedule_period_millis: u64,
        max_delegators_per_validator: Option<u32>,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
    ) -> EngineConfig {
        Self {
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            vesting_schedule_period_millis,
            max_delegators_per_validator,
            wasm_config,
            system_config,
            administrative_accounts: Default::default(),
            allow_auction_bids: DEFAULT_ALLOW_AUCTION_BIDS,
            allow_unrestricted_transfers: DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS,
            refund_handling: DEFAULT_REFUND_HANDLING,
            fee_handling: DEFAULT_FEE_HANDLING,
            compute_rewards: DEFAULT_COMPUTE_REWARDS,
        }
    }

    /// Returns the current max associated keys config.
    pub fn max_associated_keys(&self) -> u32 {
        self.max_associated_keys
    }

    /// Returns the current max runtime call stack height config.
    pub fn max_runtime_call_stack_height(&self) -> u32 {
        self.max_runtime_call_stack_height
    }

    /// Returns the current wasm config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns the current system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }

    /// Returns the minimum delegation amount in motes.
    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    /// Get the engine config's strict argument checking flag.
    pub fn strict_argument_checking(&self) -> bool {
        self.strict_argument_checking
    }

    /// Get the vesting schedule period.
    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.vesting_schedule_period_millis
    }

    /// Get the max delegators per validator
    pub fn max_delegators_per_validator(&self) -> Option<u32> {
        self.max_delegators_per_validator
    }

    /// Returns the engine config's administrative accounts.
    pub fn administrative_accounts(&self) -> &BTreeSet<AccountHash> {
        &self.administrative_accounts
    }

    /// Returns true if auction bids are allowed.
    pub fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids
    }

    /// Returns true if unrestricted transfers are allowed.
    pub fn allow_unrestricted_transfers(&self) -> bool {
        self.allow_unrestricted_transfers
    }

    /// Checks if an account hash is an administrator.
    pub(crate) fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        self.administrative_accounts.contains(account_hash)
    }

    /// Returns the engine config's refund ratio.
    pub fn refund_handling(&self) -> &RefundHandling {
        &self.refund_handling
    }

    /// Returns the engine config's fee handling strategy.
    pub fn fee_handling(&self) -> FeeHandling {
        self.fee_handling
    }

    /// Returns the engine config's compute rewards flag.
    pub fn compute_rewards(&self) -> bool {
        self.compute_rewards
    }
}

/// A builder for an [`EngineConfig`].
///
/// Any field that isn't specified will be defaulted.  See [the module docs](index.html) for the set
/// of default values.
#[derive(Default, Debug)]
pub struct EngineConfigBuilder {
    max_query_depth: Option<u64>,
    max_associated_keys: Option<u32>,
    max_runtime_call_stack_height: Option<u32>,
    minimum_delegation_amount: Option<u64>,
    strict_argument_checking: Option<bool>,
    vesting_schedule_period_millis: Option<u64>,
    max_delegators_per_validator: Option<u32>,
    wasm_config: Option<WasmConfig>,
    system_config: Option<SystemConfig>,
    administrative_accounts: Option<BTreeSet<PublicKey>>,
    allow_auction_bids: Option<bool>,
    allow_unrestricted_transfers: Option<bool>,
    refund_handling: Option<RefundHandling>,
    fee_handling: Option<FeeHandling>,
    compute_rewards: Option<bool>,
}

impl EngineConfigBuilder {
    /// Creates a new `EngineConfig` builder.
    pub fn new() -> Self {
        EngineConfigBuilder::default()
    }

    /// Sets the max query depth config option.
    pub fn with_max_query_depth(mut self, max_query_depth: u64) -> Self {
        self.max_query_depth = Some(max_query_depth);
        self
    }

    /// Sets the max associated keys config option.
    pub fn with_max_associated_keys(mut self, max_associated_keys: u32) -> Self {
        self.max_associated_keys = Some(max_associated_keys);
        self
    }

    /// Sets the max runtime call stack height config option.
    pub fn with_max_runtime_call_stack_height(
        mut self,
        max_runtime_call_stack_height: u32,
    ) -> Self {
        self.max_runtime_call_stack_height = Some(max_runtime_call_stack_height);
        self
    }

    /// Sets the strict argument checking config option.
    pub fn with_strict_argument_checking(mut self, value: bool) -> Self {
        self.strict_argument_checking = Some(value);
        self
    }

    /// Sets the vesting schedule period millis config option.
    pub fn with_vesting_schedule_period_millis(mut self, value: u64) -> Self {
        self.vesting_schedule_period_millis = Some(value);
        self
    }

    /// Sets the max delegators per validator config option.
    pub fn with_max_delegators_per_validator(mut self, value: Option<u32>) -> Self {
        self.max_delegators_per_validator = value;
        self
    }

    /// Sets the wasm config options.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Sets the system config options.
    pub fn with_system_config(mut self, system_config: SystemConfig) -> Self {
        self.system_config = Some(system_config);
        self
    }

    /// Sets the maximum wasm stack height config option.
    pub fn with_wasm_max_stack_height(mut self, wasm_stack_height: u32) -> Self {
        let wasm_config = self.wasm_config.get_or_insert_with(WasmConfig::default);
        wasm_config.max_stack_height = wasm_stack_height;
        self
    }

    /// Sets the minimum delegation amount config option.
    pub fn with_minimum_delegation_amount(mut self, minimum_delegation_amount: u64) -> Self {
        self.minimum_delegation_amount = Some(minimum_delegation_amount);
        self
    }

    /// Sets the administrative accounts.
    pub fn with_administrative_accounts(
        mut self,
        administrator_accounts: BTreeSet<PublicKey>,
    ) -> Self {
        self.administrative_accounts = Some(administrator_accounts);
        self
    }

    /// Sets the allow auction bids config option.
    pub fn with_allow_auction_bids(mut self, allow_auction_bids: bool) -> Self {
        self.allow_auction_bids = Some(allow_auction_bids);
        self
    }

    /// Sets the allow unrestricted transfers config option.
    pub fn with_allow_unrestricted_transfers(mut self, allow_unrestricted_transfers: bool) -> Self {
        self.allow_unrestricted_transfers = Some(allow_unrestricted_transfers);
        self
    }

    /// Sets the refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        match refund_handling {
            RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
                debug_assert!(
                    refund_ratio <= Ratio::one(),
                    "refund ratio should be in the range of [0, 1]"
                );
            }
        }

        self.refund_handling = Some(refund_handling);
        self
    }

    /// Sets fee handling config option.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = Some(fee_handling);
        self
    }

    /// Sets compute rewards config option.
    pub fn with_compute_rewards(mut self, compute_rewards: bool) -> Self {
        self.compute_rewards = Some(compute_rewards);
        self
    }

    /// Builds a new [`EngineConfig`] object.
    pub fn build(self) -> EngineConfig {
        let max_query_depth = self.max_query_depth.unwrap_or(DEFAULT_MAX_QUERY_DEPTH);
        let max_associated_keys = self
            .max_associated_keys
            .unwrap_or(DEFAULT_MAX_ASSOCIATED_KEYS);
        let max_runtime_call_stack_height = self
            .max_runtime_call_stack_height
            .unwrap_or(DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT);
        let minimum_delegation_amount = self
            .minimum_delegation_amount
            .unwrap_or(DEFAULT_MINIMUM_DELEGATION_AMOUNT);
        let wasm_config = self.wasm_config.unwrap_or_default();
        let system_config = self.system_config.unwrap_or_default();
        let administrative_accounts = {
            self.administrative_accounts
                .unwrap_or_default()
                .iter()
                .map(PublicKey::to_account_hash)
                .collect()
        };
        let allow_auction_bids = self
            .allow_auction_bids
            .unwrap_or(DEFAULT_ALLOW_AUCTION_BIDS);
        let allow_unrestricted_transfers = self
            .allow_unrestricted_transfers
            .unwrap_or(DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS);
        let refund_handling = self.refund_handling.unwrap_or(DEFAULT_REFUND_HANDLING);
        let fee_handling = self.fee_handling.unwrap_or(DEFAULT_FEE_HANDLING);

        let strict_argument_checking = self
            .strict_argument_checking
            .unwrap_or(DEFAULT_STRICT_ARGUMENT_CHECKING);
        let vesting_schedule_period_millis = self
            .vesting_schedule_period_millis
            .unwrap_or(DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS);
        let max_delegators_per_validator = self.max_delegators_per_validator;
        let compute_rewards = self.compute_rewards.unwrap_or(DEFAULT_COMPUTE_REWARDS);

        EngineConfig {
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            wasm_config,
            system_config,
            administrative_accounts,
            allow_auction_bids,
            allow_unrestricted_transfers,
            refund_handling,
            fee_handling,
            strict_argument_checking,
            vesting_schedule_period_millis,
            max_delegators_per_validator,
            compute_rewards,
        }
    }
}
