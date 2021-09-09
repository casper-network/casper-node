//! Implementation of balances.
use alloc::string::String;

use casper_contract::{contract_api::storage, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{bytesrepr::ToBytes, URef, U256};

use crate::{constants::BALANCES_KEY, detail, error::Error, Address};

/// Creates a dictionary item key for a dictionary item.
#[inline]
fn make_dictionary_item_key(owner: &Address) -> String {
    let preimage = owner.to_bytes().unwrap_or_revert();
    // NOTE: As for now dictionary item keys are limited to 64 characters only. Instead of using
    // hashing (which will effectively hash a hash) we'll use base64. Preimage is about 33 bytes for
    // both Address variants, and approximated base64-encoded length will be 4 * (33 / 3) ~ 44
    // characters.
    // Even if the preimage increased in size we still have extra space but even in case of much
    // larger preimage we can switch to base85 which has ratio of 4:5.
    base64::encode(&preimage)
}

pub fn get_balances_uref() -> URef {
    detail::get_uref(BALANCES_KEY)
}

/// Writes token balance of a specified account into a dictionary.
pub fn write_balance_to(balances_uref: &URef, address: &Address, amount: U256) {
    let dictionary_item_key = make_dictionary_item_key(address);
    storage::dictionary_put(*balances_uref, &dictionary_item_key, amount);
}

/// Reads token balance of a specified account.
///
/// If a given account does not have balances in the system, then a 0 is returned.
pub fn read_balance_from(balances_uref: &URef, address: &Address) -> U256 {
    let dictionary_item_key = make_dictionary_item_key(address);

    storage::dictionary_get(*balances_uref, &dictionary_item_key)
        .unwrap_or_revert()
        .unwrap_or_default()
}

/// Transfer tokens from the `sender` to the `recipient`.
///
/// This function should not be used directly by contract's entrypoint as it does not validate the
/// sender.
pub fn transfer_balance(
    balances_uref: &URef,
    sender: &Address,
    recipient: &Address,
    amount: U256,
) -> Result<(), Error> {
    let new_sender_balance = {
        let sender_balance = read_balance_from(balances_uref, sender);
        sender_balance
            .checked_sub(amount)
            .ok_or(Error::InsufficientBalance)?
    };

    let new_recipient_balance = {
        let recipient_balance = read_balance_from(balances_uref, recipient);
        recipient_balance
            .checked_add(amount)
            .ok_or(Error::Overflow)?
    };

    write_balance_to(balances_uref, sender, new_sender_balance);
    write_balance_to(balances_uref, recipient, new_recipient_balance);

    Ok(())
}
