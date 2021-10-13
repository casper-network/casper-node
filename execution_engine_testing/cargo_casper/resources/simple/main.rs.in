#![no_std]
#![no_main]

#[cfg(not(target_arch = "wasm32"))]
compile_error!("target arch should be wasm32: compile with '--target wasm32-unknown-unknown'");

// We need to explicitly import the std alloc crate and `alloc::string::String` as we're in a
// `no_std` environment.
extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, Key};

const KEY_NAME: &str = "my-key-name";
const RUNTIME_ARG_NAME: &str = "message";

/// An error enum which can be converted to a `u16` so it can be returned as an `ApiError::User`.
#[repr(u16)]
enum Error {
    KeyAlreadyExists = 0,
    KeyMismatch = 1,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

#[no_mangle]
pub extern "C" fn call() {
    // The key shouldn't already exist in the named keys.
    let missing_key = runtime::get_key(KEY_NAME);
    if missing_key.is_some() {
        runtime::revert(Error::KeyAlreadyExists);
    }

    // This contract expects a single runtime argument to be provided.  The arg is named "message"
    // and will be of type `String`.
    let value: String = runtime::get_named_arg(RUNTIME_ARG_NAME);

    // Store this value under a new unforgeable reference a.k.a `URef`.
    let value_ref = storage::new_uref(value);

    // Store the new `URef` as a named key with a name of `KEY_NAME`.
    let key = Key::URef(value_ref);
    runtime::put_key(KEY_NAME, key);

    // The key should now be able to be retrieved.  Note that if `get_key()` returns `None`, then
    // `unwrap_or_revert()` will exit the process, returning `ApiError::None`.
    let retrieved_key = runtime::get_key(KEY_NAME).unwrap_or_revert();
    if retrieved_key != key {
        runtime::revert(Error::KeyMismatch);
    }
}
