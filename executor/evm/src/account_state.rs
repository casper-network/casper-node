//! Helpers for the EVM account records stored in Casper global state.
//!
//! This module is intentionally layout-focused. It translates split Casper
//! global-state records into the account shape revm needs, but it does not
//! recover transaction signers, create Casper accounts, or decide whether an EVM
//! address should be linked to a Casper account. Those policy decisions live in
//! contract runtime and transaction validation.

use casper_storage::{tracking_copy::TrackingCopyError, TrackingCopy};
use casper_types::{account::AccountHash, evm, CLValue, EvmAddr, Key, StoredValue, URef};

/// Identity backing an EVM address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountIdentity {
    /// The EVM address is linked to a Casper account.
    ///
    /// Balance reads and writes should go through that account's main purse.
    Account(AccountHash),
    /// The EVM address is backed only by an EVM purse.
    ///
    /// This is used for EVM-native externally owned accounts and contracts that
    /// do not have a Casper account identity.
    Purse(URef),
}

/// EVM account metadata resolved into revm's account view.
///
/// This is not a stored account type. It is the adapter result used to construct
/// revm's `AccountInfo` from independent identity, nonce, and code-hash records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountMetadata {
    pub(crate) nonce: u64,
    pub(crate) code_hash: evm::Hash,
    pub(crate) main_purse: URef,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AccountStorageError {
    #[error(transparent)]
    TrackingCopy(#[from] TrackingCopyError),
    #[error("unexpected stored value for {key}: expected {expected}, found {found}")]
    TypeMismatch {
        key: Key,
        expected: &'static str,
        found: String,
    },
    #[error("failed to decode {expected} at {key}: {error}")]
    Decode {
        key: Key,
        expected: &'static str,
        error: String,
    },
    #[error("identity for {identity_key} points to missing account {account_key}")]
    MissingAccount { identity_key: Key, account_key: Key },
}

pub(crate) fn read_account_metadata<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> Result<Option<AccountMetadata>, AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    let identity = read_account_identity(tracking_copy, address)?;
    let nonce = read_nonce(tracking_copy, address)?;
    let code_hash = read_code_hash(tracking_copy, address)?;

    // No split records at all means revm should treat the account as absent.
    // A partially present record still resolves to an account: missing nonce
    // defaults to zero, missing code hash defaults to empty code, and missing
    // identity falls back to the deterministic purse for this EVM address.
    if identity.is_none() && nonce.is_none() && code_hash.is_none() {
        return Ok(None);
    }

    let main_purse = match identity {
        Some(AccountIdentity::Account(account_hash)) => {
            // A linked identity is only valid while the target Casper account
            // exists. Treat a dangling bridge as state corruption rather than
            // silently falling back to the deterministic EVM purse.
            let identity_key = Key::Evm(EvmAddr::Account(address));
            account_main_purse(tracking_copy, account_hash)?.ok_or(
                AccountStorageError::MissingAccount {
                    identity_key,
                    account_key: Key::Account(account_hash),
                },
            )?
        }
        Some(AccountIdentity::Purse(main_purse)) => main_purse,
        // This fallback lets runtime-created contract addresses or partially
        // initialized EVM-native records be read without forcing a Casper
        // account link.
        None => evm::deterministic_purse(address),
    };

    Ok(Some(AccountMetadata {
        nonce: nonce.unwrap_or(0),
        code_hash: code_hash.unwrap_or(evm::EMPTY_CODE_HASH),
        main_purse,
    }))
}

pub(crate) fn read_account_identity<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> Result<Option<AccountIdentity>, AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    let key = Key::Evm(EvmAddr::Account(address));
    match tracking_copy.read(&key)? {
        Some(StoredValue::CLValue(cl_value)) => {
            let identity_key = cl_value_to_key(key, cl_value)?;
            match identity_key {
                // The only valid identity pointer variants are a Casper account
                // hash or a purse. Nonce/code/storage are not stored here.
                Key::Account(account_hash) => Ok(Some(AccountIdentity::Account(account_hash))),
                Key::URef(uref) => Ok(Some(AccountIdentity::Purse(uref))),
                other => Err(AccountStorageError::TypeMismatch {
                    key,
                    expected: "CLValue(Key::Account) or CLValue(Key::URef)",
                    found: other.type_string(),
                }),
            }
        }
        Some(stored_value) => Err(AccountStorageError::TypeMismatch {
            key,
            expected: "StoredValue::CLValue(Key)",
            found: stored_value.type_name(),
        }),
        None => Ok(None),
    }
}

pub(crate) fn account_main_purse<R>(
    tracking_copy: &mut TrackingCopy<R>,
    account_hash: AccountHash,
) -> Result<Option<URef>, AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    let account_key = Key::Account(account_hash);
    match tracking_copy.read(&account_key)? {
        // Support both account representations because this helper is used by
        // executor reads, runtime linking, and validation against the current
        // global-state model.
        Some(StoredValue::Account(account)) => Ok(Some(account.main_purse())),
        Some(StoredValue::CLValue(cl_value)) => {
            let key = cl_value_to_key(account_key, cl_value)?;
            let Key::AddressableEntity(entity_addr) = key else {
                return Err(AccountStorageError::TypeMismatch {
                    key: account_key,
                    expected: "CLValue(Key::AddressableEntity)",
                    found: key.type_string(),
                });
            };
            let entity_key = Key::AddressableEntity(entity_addr);
            match tracking_copy.read(&entity_key)? {
                Some(StoredValue::AddressableEntity(entity)) => Ok(Some(entity.main_purse())),
                Some(stored_value) => Err(AccountStorageError::TypeMismatch {
                    key: entity_key,
                    expected: "StoredValue::AddressableEntity",
                    found: stored_value.type_name(),
                }),
                None => Err(AccountStorageError::MissingAccount {
                    identity_key: account_key,
                    account_key: entity_key,
                }),
            }
        }
        Some(stored_value) => Err(AccountStorageError::TypeMismatch {
            key: account_key,
            expected: "StoredValue::Account or StoredValue::CLValue(Key::AddressableEntity)",
            found: stored_value.type_name(),
        }),
        None => Ok(None),
    }
}

pub(crate) fn write_account_identity<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    identity_key: Key,
) -> Result<(), AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    // This helper only serializes the caller's chosen identity key. Runtime may
    // write `Key::Account`; executor state application writes `Key::URef` only
    // for EVM-native accounts and preserves existing `Key::Account` links.
    let key = Key::Evm(EvmAddr::Account(address));
    let cl_value = CLValue::from_t(identity_key).map_err(|error| AccountStorageError::Decode {
        key,
        expected: "Key",
        error: error.to_string(),
    })?;
    tracking_copy.write(key, StoredValue::CLValue(cl_value));
    Ok(())
}

pub(crate) fn write_nonce<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    nonce: u64,
) -> Result<(), AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    // Nonce is deliberately independent from the identity pointer so linking an
    // address to a Casper account does not move or rewrite EVM replay state.
    let key = Key::Evm(EvmAddr::Nonce(address));
    let cl_value = CLValue::from_t(nonce).map_err(|error| AccountStorageError::Decode {
        key,
        expected: "u64",
        error: error.to_string(),
    })?;
    tracking_copy.write(key, StoredValue::CLValue(cl_value));
    Ok(())
}

pub(crate) fn write_code_hash<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    code_hash: evm::Hash,
) -> Result<(), AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    // Code hash is deliberately independent from the identity pointer so
    // contracts can remain EVM-native even when EOAs may link to Casper
    // accounts.
    let key = Key::Evm(EvmAddr::CodeHash(address));
    let cl_value = CLValue::from_t(code_hash).map_err(|error| AccountStorageError::Decode {
        key,
        expected: "evm::Hash",
        error: error.to_string(),
    })?;
    tracking_copy.write(key, StoredValue::CLValue(cl_value));
    Ok(())
}

fn read_nonce<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> Result<Option<u64>, AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    let key = Key::Evm(EvmAddr::Nonce(address));
    match tracking_copy.read(&key)? {
        Some(StoredValue::CLValue(cl_value)) => {
            cl_value
                .into_t::<u64>()
                .map(Some)
                .map_err(|error| AccountStorageError::Decode {
                    key,
                    expected: "u64",
                    error: error.to_string(),
                })
        }
        Some(stored_value) => Err(AccountStorageError::TypeMismatch {
            key,
            expected: "StoredValue::CLValue(u64)",
            found: stored_value.type_name(),
        }),
        None => Ok(None),
    }
}

fn read_code_hash<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> Result<Option<evm::Hash>, AccountStorageError>
where
    R: casper_storage::global_state::state::StateReader<
        Key,
        StoredValue,
        Error = casper_storage::global_state::error::Error,
    >,
{
    let key = Key::Evm(EvmAddr::CodeHash(address));
    match tracking_copy.read(&key)? {
        Some(StoredValue::CLValue(cl_value)) => {
            cl_value
                .into_t::<evm::Hash>()
                .map(Some)
                .map_err(|error| AccountStorageError::Decode {
                    key,
                    expected: "evm::Hash",
                    error: error.to_string(),
                })
        }
        Some(stored_value) => Err(AccountStorageError::TypeMismatch {
            key,
            expected: "StoredValue::CLValue(evm::Hash)",
            found: stored_value.type_name(),
        }),
        None => Ok(None),
    }
}

fn cl_value_to_key(key: Key, cl_value: CLValue) -> Result<Key, AccountStorageError> {
    cl_value
        .into_t::<Key>()
        .map_err(|error| AccountStorageError::Decode {
            key,
            expected: "Key",
            error: error.to_string(),
        })
}
