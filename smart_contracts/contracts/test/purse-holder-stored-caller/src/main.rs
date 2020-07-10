#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use contract::contract_api::{runtime, storage};
use types::{runtime_args, ContractHash, RuntimeArgs};

const METHOD_VERSION: &str = "version";
const HASH_KEY_NAME: &str = "purse_holder";
const ENTRY_POINT_NAME: &str = "entry_point";
const PURSE_NAME: &str = "purse_name";

#[no_mangle]
pub extern "C" fn call() {
    let entry_point_name: String = runtime::get_named_arg(ENTRY_POINT_NAME);

    match entry_point_name.as_str() {
        METHOD_VERSION => {
            let contract_hash: ContractHash = runtime::get_named_arg(HASH_KEY_NAME);
            let version: String =
                runtime::call_contract(contract_hash, &entry_point_name, RuntimeArgs::default());
            let version_key = storage::new_uref(version).into();
            runtime::put_key(METHOD_VERSION, version_key);
        }
        _ => {
            let contract_hash: ContractHash = runtime::get_named_arg(HASH_KEY_NAME);
            let purse_name: String = runtime::get_named_arg(PURSE_NAME);

            let args = runtime_args! {
                PURSE_NAME => purse_name,
            };
            runtime::call_contract::<()>(contract_hash, &entry_point_name, args);
        }
    };
}
