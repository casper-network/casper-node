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
    AccessRights, PublicKey, URef,
};
use once_cell::sync::Lazy;

pub(crate) static SYSTEM_ACCOUNT_ADDRESS: Lazy<AccountHash> =
    Lazy::new(|| PublicKey::System.to_account_hash());

pub(crate) static VIRTUAL_SYSTEM_ACCOUNT: Lazy<Account> = Lazy::new(|| {
    let purse = URef::new(Default::default(), AccessRights::READ_ADD_WRITE);
    Account::create(*SYSTEM_ACCOUNT_ADDRESS, NamedKeys::default(), purse)
});

/// Restricted account's action thresholds
const DEFAULT_PRIVATE_CHAIN_ACTION_THRESHOLDS: ActionThresholds = ActionThresholds {
    deployment: Weight::new(1),
    key_management: Weight::MAX,
};

pub(crate) enum AccountConfig {
    /// Normal account settings for a public chain.
    Normal,
    /// Specialized account with extra settings valid only on private chains.
    Restricted,
}

/// Creates new account specific for a different chain operating modes.
pub(crate) fn create_account(
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
