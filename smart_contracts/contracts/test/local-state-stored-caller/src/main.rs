#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;
use casper_types::{ContractHash, RuntimeArgs};

const ARG_SEED: &str = "seed";
const ENTRY_FUNCTION_NAME: &str = "delegate";

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_SEED);

    runtime::call_contract(contract_hash, ENTRY_FUNCTION_NAME, RuntimeArgs::default())
}
