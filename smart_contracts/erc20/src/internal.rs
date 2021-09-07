//! Internal entry points that are not exposed by default at installation point, but can be used
//! post-install in custom contract code.
//!
//! # Security
//!
//! Those functions should never be called from entrypoints marked as public.

use casper_types::{account::AccountHash, U512};

use crate::{balances, error::Error};

/// Internal function that mints an amount of the token and assigns it to an account.
///
/// # Security
///
/// This offers no security whatsoever, and for all practical purposes user of this function is
/// advised to NOT expose this function through a public entry point.
pub fn mint(owner: &AccountHash, amount: U512) -> Result<(), Error> {
    let new_balance = {
        let balance = balances::read_balance(owner);
        balance.checked_add(amount).ok_or(Error::Overflow)?
    };
    balances::write_balance(owner, new_balance);
    Ok(())
}

/// Internal function that burns an amount of the token of a given account.
///
/// # Security
///
/// This offers no security whatsoever, and for all practical purposes user of this function is
/// advised to NOT expose this function through a public entry point.
pub fn burn(owner: &AccountHash, amount: U512) -> Result<(), Error> {
    let new_balance = {
        let balance = balances::read_balance(owner);
        balance
            .checked_sub(amount)
            .ok_or(Error::InsufficientBalance)?
    };
    balances::write_balance(owner, new_balance);
    Ok(())
}
