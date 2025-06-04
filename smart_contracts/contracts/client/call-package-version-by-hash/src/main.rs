#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::runtime;
use casper_types::{contracts::ContractPackageHash, runtime_args};

#[no_mangle]
pub extern "C" fn call() {
    let package_hash: ContractPackageHash = runtime::get_named_arg("contract_package_hash");

    runtime::call_package_version(package_hash, None, Some(1u32), "delegate", runtime_args! {})
}
