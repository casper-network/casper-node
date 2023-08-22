#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    account::AccountHash,
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys, Parameter},
    CLTyped, CLValue, Key, URef,
};

const GET_PAYMENT_PURSE_NAME: &str = "get_payment_purse";
const PACKAGE_HASH_KEY_NAME: &str = "contract_own_funds";
const HASH_KEY_NAME: &str = "contract_own_funds_hash";
const ACCESS_KEY_NAME: &str = "contract_own_funds_access";
const ARG_TARGET: &str = "target";
const CONTRACT_VERSION: &str = "contract_version";
const PAYMENT_PURSE_KEY: &str = "payment_purse";

#[no_mangle]
pub extern "C" fn get_payment_purse() {
    let purse_uref = runtime::get_key(PAYMENT_PURSE_KEY)
        .and_then(Key::into_uref)
        .unwrap_or_revert();

    let attenuated_purse = purse_uref.into_add();

    runtime::ret(CLValue::from_t(attenuated_purse).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let faucet_entrypoint = EntryPoint::new(
            GET_PAYMENT_PURSE_NAME.to_string(),
            vec![Parameter::new(ARG_TARGET, AccountHash::cl_type())],
            URef::cl_type(),
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(faucet_entrypoint);
        entry_points
    };

    let named_keys = {
        let faucet_funds = system::create_purse();

        let mut named_keys = NamedKeys::new();
        named_keys.insert(PAYMENT_PURSE_KEY.to_string(), faucet_funds.into());
        named_keys
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, contract_hash.into());
}
