#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::contract_api::runtime;
use casper_types::{runtime_args, EntityVersion, PackageAddr};

const ENTRY_FUNCTION_NAME: &str = "delegate";
const PURSE_NAME_ARG_NAME: &str = "purse_name";
const ARG_CONTRACT_PACKAGE: &str = "contract_package";
const ARG_NEW_PURSE_NAME: &str = "new_purse_name";
const ARG_MAJOR_VERSION: &str = "major_version";
const ARG_VERSION: &str = "version";

#[no_mangle]
pub extern "C" fn call() {
    let contract_package_hash: PackageAddr = runtime::get_named_arg(ARG_CONTRACT_PACKAGE);
    let new_purse_name: String = runtime::get_named_arg(ARG_NEW_PURSE_NAME);
    let major_version: u32 = runtime::get_named_arg(ARG_MAJOR_VERSION);
    let version_number: EntityVersion = runtime::get_named_arg(ARG_VERSION);

    let runtime_args = runtime_args! {
        PURSE_NAME_ARG_NAME => new_purse_name,
    };

    runtime::call_package_version(
        contract_package_hash.into(),
        Some(major_version),
        Some(version_number),
        ENTRY_FUNCTION_NAME,
        runtime_args,
    )
}
