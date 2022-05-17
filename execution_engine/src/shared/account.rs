//! Execution-related account creation helpers
//!
//! This module uses [`casper_types::account::Account`] and adds extra logic
//! specific to a public, or private chains.
//!
//! Production code should always use this module to create new account instances before writing
//! them to a global state.
use casper_types::{
    account::{Account, AccountHash, ActionThresholds, AddKeyFailure, AssociatedKeys, Weight},
    contracts::NamedKeys,
    PublicKey, URef,
};
use once_cell::sync::Lazy;

pub(crate) static SYSTEM_ACCOUNT_ADDRESS: Lazy<AccountHash> =
    Lazy::new(|| PublicKey::System.to_account_hash());

/// Restricted account's action thresholds
const DEFAULT_PRIVATE_CHAIN_ACTION_THRESHOLDS: ActionThresholds = ActionThresholds {
    deployment: Weight::new(1),
    key_management: Weight::MAX,
};

/// Variants of an account in the system.
pub enum AccountConfig {
    /// Normal account settings for a public chain.
    Normal,
    /// Specialized account with extra settings valid only on private chains.
    Restricted,
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self::Normal
    }
}

/// Creates new account specific for a different chain operating modes.
pub fn create_account(
    account_config: AccountConfig,
    account_hash: AccountHash,
    main_purse: URef,
) -> Result<Account, AddKeyFailure> {
    // Named keys are always created empty regardless of the operating mode.
    let named_keys = NamedKeys::default();

    match account_config {
        AccountConfig::Normal => return Ok(Account::create(account_hash, named_keys, main_purse)),
        AccountConfig::Restricted => {}
    }

    let associated_keys = AssociatedKeys::identity(account_hash);

    let account = Account::new(
        account_hash,
        named_keys,
        main_purse,
        associated_keys,
        DEFAULT_PRIVATE_CHAIN_ACTION_THRESHOLDS,
    );

    Ok(account)
}
