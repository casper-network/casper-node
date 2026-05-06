//! Translation from revm state changes into Casper tracking copy writes.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    KeyPrefix, TrackingCopy,
};
use casper_types::{evm, CLValue, Key, StoredValue, U512};
use revm::{
    primitives::{Address, U256},
    state::{Account, EvmState},
};

use crate::{tx, Error};

pub(crate) fn apply<R>(tracking_copy: &mut TrackingCopy<R>, state: EvmState) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    for (address, account) in state {
        apply_account(tracking_copy, address, account)?;
    }
    Ok(())
}

fn apply_account<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: Address,
    account: Account,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let address = tx::from_revm_address(address);
    let account_key = Key::EvmAccount(address);

    if account.is_selfdestructed() {
        let main_purse = existing_main_purse(tracking_copy, &account_key)?
            .unwrap_or_else(|| evm::deterministic_purse(address));
        prune_account(tracking_copy, address, account_key, main_purse)?;
        return Ok(());
    }

    if let Some(code) = account.info.code.as_ref() {
        let bytes = code.original_byte_slice();
        if !bytes.is_empty() {
            tracking_copy.write(
                Key::EvmByteCode(tx::from_revm_hash(account.info.code_hash)),
                StoredValue::EvmByteCode(evm::ByteCode::new(bytes.to_vec())),
            );
        }
    }

    let main_purse = existing_main_purse(tracking_copy, &account_key)?
        .unwrap_or_else(|| evm::deterministic_purse(address));
    let code_hash = tx::from_revm_hash(account.info.code_hash);

    tracking_copy.write(
        account_key,
        StoredValue::EvmAccount(evm::Account::new(account.info.nonce, code_hash, main_purse)),
    );
    write_balance(tracking_copy, main_purse, account.info.balance)?;

    for (slot, value) in account.changed_storage_slots() {
        let key = Key::EvmStorage(evm::StorageAddr::new(address, tx::from_revm_u256(*slot)));
        if value.present_value.is_zero() {
            tracking_copy.prune(key);
        } else {
            tracking_copy.write(
                key,
                StoredValue::EvmStorage(evm::StorageValue::new(tx::from_revm_u256(
                    value.present_value,
                ))),
            );
        }
    }

    Ok(())
}

fn prune_account<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    account_key: Key,
    main_purse: casper_types::URef,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let storage_keys = tracking_copy
        .get_keys_by_prefix(&KeyPrefix::EvmStorageByAddress(address))
        .map_err(|error| Error::State(error.to_string()))?;
    for key in storage_keys {
        tracking_copy.prune(key);
    }
    tracking_copy.prune(Key::Balance(main_purse.addr()));
    tracking_copy.prune(account_key);
    Ok(())
}

fn existing_main_purse<R>(
    tracking_copy: &mut TrackingCopy<R>,
    account_key: &Key,
) -> Result<Option<casper_types::URef>, Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    match tracking_copy
        .read(account_key)
        .map_err(|error| Error::State(error.to_string()))?
    {
        Some(StoredValue::EvmAccount(account)) => Ok(Some(account.main_purse())),
        Some(stored_value) => Err(Error::State(format!(
            "unexpected stored value for {account_key}: expected StoredValue::EvmAccount, found {}",
            stored_value.type_name()
        ))),
        None => Ok(None),
    }
}

fn write_balance<R>(
    tracking_copy: &mut TrackingCopy<R>,
    main_purse: casper_types::URef,
    balance: U256,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let balance = u256_to_u512(balance);
    let cl_value = CLValue::from_t(balance).map_err(|error| Error::State(error.to_string()))?;
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(cl_value),
    );
    Ok(())
}

fn u256_to_u512(value: U256) -> U512 {
    let bytes = value.to_be_bytes::<32>();
    U512::from_big_endian(&bytes)
}
