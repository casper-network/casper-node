#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::contract_api::{runtime, storage, system};
use casperlabs_types::{Key, RuntimeArgs};

const NEW_ENDPOINT_NAME: &str = "version";
const RESULT_UREF_NAME: &str = "output_version";

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash = system::get_mint();
    let value: String =
        runtime::call_contract(contract_hash, NEW_ENDPOINT_NAME, RuntimeArgs::default());
    runtime::put_key(RESULT_UREF_NAME, Key::URef(storage::new_uref(value)));
}
