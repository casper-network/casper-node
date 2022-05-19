use casper_execution_engine::{
    core::engine_state::{
        engine_config::{
            FeeHandling, RefundHandling, DEFAULT_ALLOW_AUCTION_BIDS,
            DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS, DEFAULT_FEE_HANDLING,
            DEFAULT_MAX_ASSOCIATED_KEYS, DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            DEFAULT_REFUND_HANDLING, DEFAULT_STRICT_ARGUMENT_CHECKING,
        },
        genesis::AdministratorAccount,
        EngineConfig, DEFAULT_MAX_QUERY_DEPTH, DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
    },
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use num_rational::Ratio;
use num_traits::One;

/// This is a builder pattern applied to the [`EngineConfig`] structure to shield any changes to the
/// constructor, or contents of it from the rest of the system.
///
/// Any field that isn't specified will be defaulted.
#[derive(Default, Debug)]
pub struct EngineConfigBuilder {
    max_query_depth: Option<u64>,
    max_associated_keys: Option<u32>,
    max_runtime_call_stack_height: Option<u32>,
    wasm_config: Option<WasmConfig>,
    system_config: Option<SystemConfig>,
    minimum_delegation_amount: Option<u64>,
    strict_argument_checking: Option<bool>,
    administrative_accounts: Option<Vec<AdministratorAccount>>,
    allow_auction_bids: Option<bool>,
    allow_unrestricted_transfers: Option<bool>,
    refund_handling: Option<RefundHandling>,
    fee_handling: Option<FeeHandling>,
}

impl EngineConfigBuilder {
    /// Create new `EngineConfig` builder object.
    pub fn new() -> Self {
        EngineConfigBuilder::default()
    }

    /// Set a max query depth config option.
    pub fn with_max_query_depth(mut self, max_query_depth: u64) -> Self {
        self.max_query_depth = Some(max_query_depth);
        self
    }

    /// Set a max associated keys config option.
    pub fn with_max_associated_keys(mut self, max_associated_keys: u32) -> Self {
        self.max_associated_keys = Some(max_associated_keys);
        self
    }

    /// Set a max runtime call stack height option.
    pub fn with_max_runtime_call_stack_height(
        mut self,
        max_runtime_call_stack_height: u32,
    ) -> Self {
        self.max_runtime_call_stack_height = Some(max_runtime_call_stack_height);
        self
    }

    /// Set a new wasm config configuration option.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Set a new system config configuration option.
    pub fn with_system_config(mut self, system_config: SystemConfig) -> Self {
        self.system_config = Some(system_config);
        self
    }

    /// Sets new maximum wasm memory.
    pub fn with_wasm_max_stack_height(mut self, wasm_stack_height: u32) -> Self {
        let wasm_config = self.wasm_config.get_or_insert_with(WasmConfig::default);
        wasm_config.max_stack_height = wasm_stack_height;
        self
    }

    /// Set a new system config configuration option.
    pub fn with_minimum_delegation_amount(mut self, minimum_delegation_amount: u64) -> Self {
        self.minimum_delegation_amount = Some(minimum_delegation_amount);
        self
    }

    /// Sets new maximum wasm memory.
    pub fn with_strict_argument_checking(mut self, strict_argument_checking: bool) -> Self {
        self.strict_argument_checking = Some(strict_argument_checking);
        self
    }

    /// Sets new chain kind.
    pub fn with_administrative_accounts(
        mut self,
        administrator_accounts: Vec<AdministratorAccount>,
    ) -> Self {
        self.administrative_accounts = Some(administrator_accounts);
        self
    }

    /// Sets new disable auction bids flag.
    pub fn with_allow_auction_bids(mut self, allow_auction_bids: bool) -> Self {
        self.allow_auction_bids = Some(allow_auction_bids);
        self
    }

    /// Set the engine config builder's allow unrestricted transfers.
    pub fn with_allow_unrestricted_transfers(mut self, allow_unrestricted_transfers: bool) -> Self {
        self.allow_unrestricted_transfers = Some(allow_unrestricted_transfers);
        self
    }

    /// Set the engine config builder's refund handling.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        match refund_handling {
            RefundHandling::Refund { refund_ratio } => {
                debug_assert!(
                    refund_ratio <= Ratio::one(),
                    "refund ratio should be a proper fraction"
                );
            }
        }

        self.refund_handling = Some(refund_handling);
        self
    }

    /// Set the engine config builder's fee handling.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = Some(fee_handling);
        self
    }

    /// Build a new [`EngineConfig`] object.
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
        let strict_argument_checking = self
            .strict_argument_checking
            .unwrap_or(DEFAULT_STRICT_ARGUMENT_CHECKING);
        let wasm_config = self.wasm_config.unwrap_or_default();
        let system_config = self.system_config.unwrap_or_default();
        let administrative_accounts = self.administrative_accounts.unwrap_or_default();
        let allow_auction_bids = self
            .allow_auction_bids
            .unwrap_or(DEFAULT_ALLOW_AUCTION_BIDS);
        let allow_unrestricted_transfers = self
            .allow_unrestricted_transfers
            .unwrap_or(DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS);
        let refund_handling = self.refund_handling.unwrap_or(DEFAULT_REFUND_HANDLING);
        let fee_handling = self.fee_handling.unwrap_or(DEFAULT_FEE_HANDLING);

        EngineConfig::new(
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            wasm_config,
            system_config,
            administrative_accounts,
            allow_auction_bids,
            allow_unrestricted_transfers,
            refund_handling,
            fee_handling,
        )
    }
}
