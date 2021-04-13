#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, U512};

#[repr(u16)]
enum Error {
    NotEqual,
}

#[no_mangle]
pub extern "C" fn call() {
    let initial_value: U512 = U512::one();

    let uref = storage::new_uref(initial_value);

    let maybe_value: Option<U512> = storage::read(uref).unwrap_or_revert();

    if maybe_value != Some(initial_value) {
        runtime::revert(ApiError::User(Error::NotEqual as u16))
    }

    storage::delete(uref);

    let maybe_value: Option<U512> = storage::read(uref).unwrap_or_revert();

    if maybe_value.is_some() {
        runtime::revert(ApiError::User(Error::NotEqual as u16))
    }

    storage::delete(uref);

    let maybe_value: Option<U512> = storage::read(uref).unwrap_or_revert();

    if maybe_value.is_some() {
        runtime::revert(ApiError::User(Error::NotEqual as u16))
    }
}
