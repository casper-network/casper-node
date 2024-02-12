#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{AddressableEntityHash, ApiError, RuntimeArgs, URef};

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash: AddressableEntityHash = runtime::get_key("hello_ext")
        .unwrap_or_revert_with(ApiError::GetKey)
        .into_entity_hash_addr()
        .unwrap_or_revert()
        .into();

    let result: URef = runtime::call_contract(contract_hash, "hello_ext", RuntimeArgs::default());

    let value = storage::read(result);

    assert_eq!(Ok(Some("Hello, world!".to_string())), value);
}
