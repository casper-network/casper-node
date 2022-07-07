#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, Key};

const DICTIONARY_NAME: &str = "dictionary-name";
const DICTIONARY_ITEM_KEY: &str = "dictionary-item-key";
const DICTIONARY_VALUE: &str = "dictionary-value";

#[no_mangle]
pub extern "C" fn call() {
    let dictionary_seed_uref = storage::new_dictionary(DICTIONARY_NAME).unwrap_or_revert();
    storage::dictionary_put(
        dictionary_seed_uref,
        DICTIONARY_ITEM_KEY,
        DICTIONARY_VALUE.to_string(),
    );
    let dictionary_address_key =
        Key::dictionary(dictionary_seed_uref, DICTIONARY_ITEM_KEY.as_bytes());
    let value_via_read = storage::read_from_key::<String>(dictionary_address_key)
        .unwrap_or_revert()
        .unwrap_or_revert();
    let value_via_get: String = storage::dictionary_get(dictionary_seed_uref, DICTIONARY_ITEM_KEY)
        .unwrap_or_revert()
        .unwrap_or_revert();
    if value_via_read != *DICTIONARY_VALUE {
        runtime::revert(ApiError::User(16u16))
    }
    if value_via_get != value_via_read {
        runtime::revert(ApiError::User(17u16))
    }
}
