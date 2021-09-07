//! Implementation of a ERC20 Token Standard.
#![warn(missing_docs)]
#![feature(once_cell)]
#![no_std]

extern crate alloc;

pub mod allowances;
pub mod balances;
pub mod constants;
mod detail;
pub mod entry_points;
pub mod error;
pub mod internal;

use alloc::string::{String, ToString};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, contracts::NamedKeys, Key, U512};

use constants::{
    ALLOWANCES_KEY, BALANCES_KEY, CONTRACT_KEY, DECIMALS_KEY, NAME_KEY, SYMBOL_KEY,
    TOTAL_SUPPLY_KEY,
};
use error::Error;

/// Returns name of the token.
pub fn name() -> String {
    detail::read_from(NAME_KEY)
}

/// Returns symbol of the token.
pub fn symbol() -> String {
    detail::read_from(SYMBOL_KEY)
}

/// Returns decimals of the token.
pub fn decimals() -> u8 {
    detail::read_from(DECIMALS_KEY)
}

/// Returns total supply of the token.
pub fn total_supply() -> U512 {
    detail::read_from(TOTAL_SUPPLY_KEY)
}

/// Checks balance of an owner.
pub fn balance_of(owner: AccountHash) -> U512 {
    balances::read_balance(&owner)
}

/// Transfer tokens from the caller to the `recipient`.
pub fn transfer(recipient: &AccountHash, amount: U512) -> Result<(), Error> {
    let sender = detail::get_immediate_session_caller()?;

    balances::transfer_balance(&sender, recipient, amount)
}

/// Allow other address to transfer caller's tokens.
pub fn approve(spender: AccountHash, amount: U512) -> Result<(), Error> {
    let owner = detail::get_immediate_session_caller()?;

    allowances::write_allowance(&owner, &spender, amount);

    Ok(())
}

/// Returns the amount allowed to spend.
pub fn allowance(owner: AccountHash, spender: AccountHash) -> U512 {
    allowances::read_allowance(&owner, &spender)
}

/// Transfer tokens from `owner` address to the `recipient` address if required `amount` was
/// approved before to be spend by the direct caller.
///
/// This operation should decrement approved amount on the `owner`, and increase balance on the
/// `recipient`.
pub fn transfer_from(
    owner: AccountHash,
    recipient: AccountHash,
    amount: U512,
) -> Result<(), Error> {
    let spender = detail::get_immediate_session_caller()?;

    let new_spender_allowance = {
        let spender_allowance = allowances::read_allowance(&owner, &spender);
        spender_allowance
            .checked_sub(amount)
            .ok_or(Error::InsufficientAllowance)?
    };

    balances::transfer_balance(&owner, &recipient, amount)?;

    allowances::write_allowance(&owner, &spender, new_spender_allowance);

    Ok(())
}

/// This is the main entry point of the contract.
///
/// It should be called from within `fn call` of your contract.
pub fn delegate(
    name: String,
    symbol: String,
    decimals: u8,
    initial_supply: U512,
) -> Result<(), Error> {
    // Only a session code can call into this, and attempt to call this function from within stored
    // contracts of any type will raise an error.
    detail::requires_session_caller()?;

    let entry_points = entry_points::get_entry_points();

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let name_key = {
            let name_uref = storage::new_uref(name).into_read();
            Key::from(name_uref)
        };

        let symbol_key = {
            let symbol_uref = storage::new_uref(symbol).into_read();
            Key::from(symbol_uref)
        };

        let decimals_key = {
            let decimals_uref = storage::new_uref(decimals).into_read();
            Key::from(decimals_uref)
        };

        let total_supply_key = {
            let total_supply_uref = storage::new_uref(initial_supply).into_read();
            Key::from(total_supply_uref)
        };

        let balances_dictionary_key = {
            let balances_uref = storage::new_dictionary(BALANCES_KEY).unwrap_or_revert();

            // Sets up initial balance for the caller.
            balances::write_balance_into(balances_uref, &runtime::get_caller(), initial_supply);

            runtime::remove_key(BALANCES_KEY);

            Key::from(balances_uref)
        };

        let allowances_dictionary_key = {
            let allowance_uref = storage::new_dictionary(ALLOWANCES_KEY).unwrap_or_revert();
            runtime::remove_key(ALLOWANCES_KEY);

            Key::from(allowance_uref)
        };

        named_keys.insert(NAME_KEY.to_string(), name_key);
        named_keys.insert(SYMBOL_KEY.to_string(), symbol_key);
        named_keys.insert(DECIMALS_KEY.to_string(), decimals_key);
        named_keys.insert(BALANCES_KEY.to_string(), balances_dictionary_key);
        named_keys.insert(ALLOWANCES_KEY.to_string(), allowances_dictionary_key);
        named_keys.insert(TOTAL_SUPPLY_KEY.to_string(), total_supply_key);

        named_keys
    };

    let (contract_hash, _version) =
        storage::new_locked_contract(entry_points, Some(named_keys), None, None);

    // Hash of the installed contract will be reachable through named keys.
    runtime::put_key(CONTRACT_KEY, Key::from(contract_hash));

    Ok(())
}
