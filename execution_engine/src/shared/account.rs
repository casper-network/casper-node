//! Execution-related account creation helpers
//!
//! This module uses [`casper_types::account::Account`] and adds extra logic
//! specific to a public, or private chains.
//!
//! Production code should always use this module to create new account instances before writing
//! them to a global state.
use std::collections::BTreeSet;

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

pub(crate) enum AccountKind {
    /// Standard account settings for a public chain.
    Public,
    /// Specialized account with extra settings valid only on private chains.
    Private {
        administrative_accounts: BTreeSet<AccountHash>,
    },
}

/// Creates new account specific for a different chain operating modes.
pub(crate) fn create_account(
    account_kind: AccountKind,
    account_hash: AccountHash,
    main_purse: URef,
) -> Result<Account, AddKeyFailure> {
    // Named keys are always created empty regardless of the operating mode.
    let named_keys = NamedKeys::default();

    let administrative_accounts = match account_kind {
        AccountKind::Public => return Ok(Account::create(account_hash, named_keys, main_purse)),
        AccountKind::Private {
            administrative_accounts,
        } => administrative_accounts,
    };

    let mut associated_keys = AssociatedKeys::identity(account_hash);

    for admin_account_hash in administrative_accounts {
        match associated_keys.add_key(admin_account_hash, Weight::MAX) {
            Ok(()) => {}
            Err(AddKeyFailure::DuplicateKey) if admin_account_hash == account_hash => {
                // We're creating a special account itself and associated key already
                // exists for it.
            }
            Err(error) => return Err(error),
        }
    }

    let account = Account::new(
        account_hash,
        named_keys,
        main_purse,
        associated_keys,
        DEFAULT_PRIVATE_CHAIN_ACTION_THRESHOLDS,
    );

    Ok(account)
}
