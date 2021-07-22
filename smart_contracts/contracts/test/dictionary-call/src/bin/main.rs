#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use core::str::FromStr;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{bytesrepr::FromBytes, CLTyped, ContractHash, RuntimeArgs, URef};

use dictionary::{
    DEFAULT_DICTIONARY_NAME, DEFAULT_DICTIONARY_VALUE, INVALID_GET_DICTIONARY_ITEM_KEY_ENTRYPOINT,
    INVALID_PUT_DICTIONARY_ITEM_KEY_ENTRYPOINT,
};
use dictionary_call::{
    Operation, ARG_CONTRACT_HASH, ARG_FORGED_UREF, ARG_OPERATION, ARG_SHARE_UREF_ENTRYPOINT,
    NEW_DICTIONARY_ITEM_KEY, NEW_DICTIONARY_VALUE,
};

/// Calls dictionary contract by hash as passed by `ARG_CONTRACT_HASH` argument and returns a
/// single value.
fn call_dictionary_contract<T: CLTyped + FromBytes>(entrypoint: &str) -> T {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
    runtime::call_contract(contract_hash, entrypoint, RuntimeArgs::default())
}

#[no_mangle]
pub extern "C" fn call() {
    let operation = {
        let arg_operation: String = runtime::get_named_arg(ARG_OPERATION);
        Operation::from_str(&arg_operation).unwrap_or_revert()
    };

    match operation {
        Operation::Write => {
            let entrypoint: String = runtime::get_named_arg(ARG_SHARE_UREF_ENTRYPOINT);
            let uref = call_dictionary_contract(&entrypoint);
            let value: String = NEW_DICTIONARY_VALUE.to_string();
            storage::dictionary_put(uref, NEW_DICTIONARY_ITEM_KEY, value);
        }
        Operation::Read => {
            let entrypoint: String = runtime::get_named_arg(ARG_SHARE_UREF_ENTRYPOINT);
            let uref = call_dictionary_contract(&entrypoint);
            let maybe_value =
                storage::dictionary_get(uref, DEFAULT_DICTIONARY_NAME).unwrap_or_revert();
            // Whether the value exists or not we're mostly interested in validation of access
            // rights
            let value: String = maybe_value.unwrap_or_default();
            assert_eq!(value, DEFAULT_DICTIONARY_VALUE);
        }
        Operation::ForgedURefWrite => {
            let uref: URef = runtime::get_named_arg(ARG_FORGED_UREF);
            let value: String = NEW_DICTIONARY_VALUE.to_string();
            storage::dictionary_put(uref, NEW_DICTIONARY_ITEM_KEY, value);
        }
        Operation::InvalidPutDictionaryItemKey => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
            runtime::call_contract(
                contract_hash,
                INVALID_PUT_DICTIONARY_ITEM_KEY_ENTRYPOINT,
                RuntimeArgs::default(),
            )
        }
        Operation::InvalidGetDictionaryItemKey => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
            runtime::call_contract(
                contract_hash,
                INVALID_GET_DICTIONARY_ITEM_KEY_ENTRYPOINT,
                RuntimeArgs::default(),
            )
        }
    }
}
