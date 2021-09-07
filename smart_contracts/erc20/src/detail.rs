//! Implementation details.
use core::convert::TryInto;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, bytesrepr::FromBytes, system::CallStackElement, CLTyped, URef,
};

use crate::error::Error;

/// Gets [`URef`] under a name.
pub fn get_uref(name: &str) -> URef {
    let key = runtime::get_key(name).unwrap_or_revert();
    key.try_into().unwrap_or_revert()
}

/// Reads value from a named key.
pub fn read_from<T>(name: &str) -> T
where
    T: FromBytes + CLTyped,
{
    let uref = get_uref(name);
    let value: T = storage::read(uref).unwrap_or_revert().unwrap_or_revert();
    value
}

/// Gets the immediate call stack element of the current execution.
fn get_immediate_call_stack_item() -> Option<CallStackElement> {
    let call_stack = runtime::get_call_stack();
    call_stack.into_iter().rev().nth(1)
}

/// Gets the immediate session caller of the current execution.
///
/// This function ensures that only session code can execute this function, and disallows stored
/// session/stored contracts.
#[inline]
pub fn get_immediate_session_caller() -> Result<AccountHash, Error> {
    match get_immediate_call_stack_item() {
        Some(CallStackElement::Session { account_hash }) => Ok(account_hash),
        Some(CallStackElement::StoredSession { .. })
        | Some(CallStackElement::StoredContract { .. })
        | None => Err(Error::InvalidContext),
    }
}

/// This function makes sure that the contract is called directly through a deploy.
///
/// An attempt to call this function from within a stored contract will fail with
/// [`Error::InvalidContext`].
#[inline]
pub fn requires_session_caller() -> Result<(), Error> {
    let call_stack = runtime::get_call_stack();

    if let Some(CallStackElement::Session { .. }) = call_stack.into_iter().rev().next() {
        // Only session code is allowed
        Ok(())
    } else {
        Err(Error::InvalidContext)
    }
}
