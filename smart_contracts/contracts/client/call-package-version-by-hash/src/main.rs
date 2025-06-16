#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::contract_api::runtime;
use casper_types::{contracts::ContractPackageHash, runtime_args};

const ARG_PURSE_NAME: &str = "purse_name";

#[no_mangle]
pub extern "C" fn call() {
    let package_hash: ContractPackageHash = runtime::get_named_arg("contract_package_hash");
    let entity_version: Option<u32> = runtime::get_named_arg("version");
    let major_version: Option<u32> = runtime::get_named_arg("major_version");
    let entry_point_name: String = runtime::get_named_arg("entry_point");
    let purse_name: String = runtime::get_named_arg(ARG_PURSE_NAME);

    runtime::call_package_version(
        package_hash,
        major_version,
        entity_version,
        &entry_point_name,
        runtime_args! {
            ARG_PURSE_NAME => purse_name
        },
    )
}
