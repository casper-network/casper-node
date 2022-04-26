//! Support for runtime configuration of the execution engine - as an integral property of the
//! `EngineState` instance.
use casper_types::account::AccountHash;

use crate::shared::{system_config::SystemConfig, wasm_config::WasmConfig};

use super::genesis::AdministratorAccount;

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;
/// Default value for minimum delegation amount in motes.
pub const DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64 = 500 * 1_000_000_000;
/// Default value for strict argument checking.
pub const DEFAULT_STRICT_ARGUMENT_CHECKING: bool = false;
/// Default value for allowing auction bids.
pub const DEFAULT_ALLOW_AUCTION_BIDS: bool = true;
/// Default value for allowing p2p transfers
pub const DEFAULT_ALLOW_P2P_TRANSFERS: bool = true;

///
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
    wasm_config: WasmConfig,
    system_config: SystemConfig,
    /// A private network specifies a list of administrative accounts.
    administrative_accounts: Vec<AdministratorAccount>,
    /// Auction entrypoints such as "add_bid" or "delegate" are disabled if this flag is set to
    /// `true`.
    allow_auction_bids: bool,
    /// Allow p2p transfers between normal accounts.
    ///
    /// If set to `true` accounts can transfer tokens between themselves without restrictions (aka
    /// public chain mode). If set to `false` tokens can be transferred only from normal
    /// accounts to administrators and administrators to normal accounts but not normal accounts to
    /// normal accounts (aka private chain mode).
    allow_p2p_transfers: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
            max_associated_keys: DEFAULT_MAX_ASSOCIATED_KEYS,
            max_runtime_call_stack_height: DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
            minimum_delegation_amount: DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            strict_argument_checking: DEFAULT_STRICT_ARGUMENT_CHECKING,
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
            administrative_accounts: Vec::default(),
            allow_auction_bids: DEFAULT_ALLOW_AUCTION_BIDS,
            allow_p2p_transfers: DEFAULT_ALLOW_P2P_TRANSFERS,
        }
    }
}

impl EngineConfig {
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

    /// Get the engine config's administrative accounts.
    #[must_use]
    pub fn administrative_accounts(&self) -> &Vec<AdministratorAccount> {
        &self.administrative_accounts
    }

    /// Checks if chain is configured in private mode.
    #[must_use]
    pub fn is_private_chain(&self) -> bool {
        !self.administrative_accounts().is_empty()
    }

    /// Get the engine config's allow auction bids.
    #[must_use]
    pub fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids
    }

    /// Get the engine config's allow p2p transfers.
    #[must_use]
    pub fn allow_p2p_transfers(&self) -> bool {
        self.allow_p2p_transfers
    }

    /// Checks if an account hash is an administrator.
    ///
    /// This method returns a `None` if chain is not configured in a private chain. Otherwise Some
    /// with a value whether passed account hash is an admin.
    #[must_use]
    pub(crate) fn is_account_administrator(&self, account_hash: &AccountHash) -> Option<bool> {
        let mut admins = self.administrative_accounts().iter().peekable();

        // Ensure it's a private chain and there's at least one administrator configured.
        admins.peek()?;

        // Find an administrator by its public key.
        let has_admin_account_hash =
            admins.any(|admin| &admin.public_key().to_account_hash() == account_hash);
        Some(has_admin_account_hash)
    }
}

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
    allow_p2p_transfers: Option<bool>,
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

    /// Set the engine config builder's allow p2p transfers.
    pub fn with_allow_p2p_transfers(mut self, allow_p2p_transfers: bool) -> Self {
        self.allow_p2p_transfers = Some(allow_p2p_transfers);
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
        let allow_p2p_transfers = self
            .allow_p2p_transfers
            .unwrap_or(DEFAULT_ALLOW_P2P_TRANSFERS);
        EngineConfig {
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            minimum_delegation_amount,
            strict_argument_checking,
            wasm_config,
            system_config,
            administrative_accounts,
            allow_auction_bids,
            allow_p2p_transfers,
        }
    }
}
