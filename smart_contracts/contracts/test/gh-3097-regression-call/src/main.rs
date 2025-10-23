#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{
    contracts::{ContractHash, ContractPackageHash, ContractVersion},
    ApiError, RuntimeArgs,
};

const CONTRACT_PACKAGE_HASH_KEY: &str = "contract_package_hash";
const DO_SOMETHING_ENTRYPOINT: &str = "do_something";
const ARG_METHOD: &str = "method";
const ARG_CONTRACT_HASH_KEY: &str = "contract_hash_key";
const ARG_MAJOR_VERSION: &str = "major_version";
const ARG_CONTRACT_VERSION: &str = "contract_version";
const METHOD_CALL_CONTRACT: &str = "call_contract";
const METHOD_CALL_VERSIONED_CONTRACT: &str = "call_versioned_contract";

#[no_mangle]
pub extern "C" fn call() {
    let method: String = runtime::get_named_arg(ARG_METHOD);
    if method == METHOD_CALL_CONTRACT {
        let contract_hash_key_name: String = runtime::get_named_arg(ARG_CONTRACT_HASH_KEY);
        let contract_hash = runtime::get_key(&contract_hash_key_name)
            .ok_or(ApiError::MissingKey)
            .unwrap_or_revert()
            .into_entity_hash_addr()
            .ok_or(ApiError::UnexpectedKeyVariant)
            .map(ContractHash::new)
            .unwrap_or_revert();
        runtime::call_contract::<()>(
            contract_hash,
            DO_SOMETHING_ENTRYPOINT,
            RuntimeArgs::default(),
        )
    } else if method == METHOD_CALL_VERSIONED_CONTRACT {
        let contract_package_hash = runtime::get_key(CONTRACT_PACKAGE_HASH_KEY)
            .ok_or(ApiError::MissingKey)
            .unwrap_or_revert()
            .into_package_addr()
            .ok_or(ApiError::UnexpectedKeyVariant)
            .map(|package_addr| ContractPackageHash::new(package_addr.value()))
            .unwrap_or_revert();

        let major_version = runtime::get_named_arg(ARG_MAJOR_VERSION);
        let contract_version =
            runtime::get_named_arg::<Option<ContractVersion>>(ARG_CONTRACT_VERSION);
        match contract_version {
            None => {
                runtime::call_versioned_contract::<()>(
                    contract_package_hash,
                    None,
                    DO_SOMETHING_ENTRYPOINT,
                    RuntimeArgs::default(),
                );
            }
            Some(contract_version) => {
                runtime::call_package_version::<()>(
                    contract_package_hash,
                    Some(major_version),
                    Some(contract_version),
                    DO_SOMETHING_ENTRYPOINT,
                    RuntimeArgs::default(),
                );
            }
        }
    } else {
        runtime::revert(ApiError::User(0));
    }
}
