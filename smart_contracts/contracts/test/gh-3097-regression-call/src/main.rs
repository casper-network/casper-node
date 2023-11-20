#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{AddressableEntityHash, ApiError, PackageHash, RuntimeArgs};

const CONTRACT_PACKAGE_HASH_KEY: &str = "contract_package_hash";
const DO_SOMETHING_ENTRYPOINT: &str = "do_something";
const ARG_METHOD: &str = "method";
const ARG_CONTRACT_HASH_KEY: &str = "contract_hash_key";
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
            .map(AddressableEntityHash::new)
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
            .map(PackageHash::new)
            .unwrap_or_revert();

        let contract_version = runtime::get_named_arg(ARG_CONTRACT_VERSION);
        runtime::call_versioned_contract::<()>(
            contract_package_hash,
            contract_version,
            DO_SOMETHING_ENTRYPOINT,
            RuntimeArgs::default(),
        );
    } else {
        runtime::revert(ApiError::User(0));
    }
}
