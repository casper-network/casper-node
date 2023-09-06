#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ContractHash, ContractPackageHash};

const ARG_CONTRACT_PACKAGE_HASH: &str = "contract_package_hash";
const ARG_CONTRACT_HASH: &str = "contract_hash";

#[no_mangle]
pub extern "C" fn call() {
    // This contract can be run only by an administrator account.
    let contract_package_hash: ContractPackageHash =
        runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

    storage::disable_contract_version(contract_package_hash, contract_hash).unwrap_or_revert();
}
