#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::runtime;
use casper_types::{contracts::ContractPackageHash, runtime_args};

#[no_mangle]
pub extern "C" fn call() {
    let package_hash: ContractPackageHash = runtime::get_named_arg("contract_package_hash");
    let major_version: u32 = runtime::get_named_arg("major_version");
    let version: u32 = runtime::get_named_arg("version");

    runtime::call_package_version(
        package_hash,
        Some(major_version),
        Some(version),
        "delegate",
        runtime_args! {},
    )
}
