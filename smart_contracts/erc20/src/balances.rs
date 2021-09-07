//! Implementation of balances.
use alloc::string::String;

use casper_contract::{contract_api::storage, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{account::AccountHash, URef, U512};

use crate::{constants::BALANCES_KEY, detail, error::Error};

/// Creates a dictionary item key for a dictionary item.
#[inline]
fn make_dictionary_item_key(owner: &AccountHash) -> String {
    hex::encode(owner.as_bytes())
}

static mut BALANCES_UREF: Option<URef> = None;

fn get_balances_uref() -> URef {
    // TODO: unsafe impl Sync for URef {}
    unsafe { *BALANCES_UREF.get_or_insert_with(|| detail::get_uref(BALANCES_KEY)) }
}

/// Writes token balance of a specified account.
pub fn write_balance(account_hash: &AccountHash, amount: U512) {
    let balances_uref = get_balances_uref();
    write_balance_into(balances_uref, account_hash, amount);
}

/// Writes token balance of a specified account into a dictionary.
pub fn write_balance_into(balances_uref: URef, account_hash: &AccountHash, amount: U512) {
    let dictionary_item_key = make_dictionary_item_key(account_hash);
    storage::dictionary_put(balances_uref, &dictionary_item_key, amount);
}

/// Reads token balance of a specified account.
///
/// If a given account does not have balances in the system, then a 0 is returned.
pub fn read_balance(account_hash: &AccountHash) -> U512 {
    let balances_uref = get_balances_uref();
    let dictionary_item_key = make_dictionary_item_key(account_hash);

    storage::dictionary_get(balances_uref, &dictionary_item_key)
        .unwrap_or_revert()
        .unwrap_or_default()
}

/// Transfer tokens from the `sender` to the `recipient`.
///
/// This function should not be used directly by contract's entrypoint as it does not validate the
/// sender.
pub fn transfer_balance(
    sender: &AccountHash,
    recipient: &AccountHash,
    amount: U512,
) -> Result<(), Error> {
    let new_sender_balance = {
        let sender_balance = read_balance(sender);
        sender_balance
            .checked_sub(amount)
            .ok_or(Error::InsufficientBalance)?
    };

    let new_recipient_balance = {
        let recipient_balance = read_balance(recipient);
        recipient_balance
            .checked_add(amount)
            .ok_or(Error::Overflow)?
    };

    write_balance(sender, new_sender_balance);
    write_balance(recipient, new_recipient_balance);

    Ok(())
}
