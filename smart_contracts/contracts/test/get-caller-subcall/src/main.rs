#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, CLType, CLValue, EntryPoint, EntryPointAccess, EntryPointPayment,
    EntryPointType, EntryPoints, RuntimeArgs,
};

const ENTRY_POINT_NAME: &str = "get_caller_ext";
const HASH_KEY_NAME: &str = "caller_subcall";
const ACCESS_KEY_NAME: &str = "caller_subcall_access";
const ARG_ACCOUNT: &str = "account";

#[no_mangle]
pub extern "C" fn get_caller_ext() {
    let caller_account_hash: AccountHash = runtime::get_caller();
    runtime::ret(CLValue::from_t(caller_account_hash).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn call() {
    let known_account_hash: AccountHash = runtime::get_named_arg(ARG_ACCOUNT);
    let caller_account_hash: AccountHash = runtime::get_caller();
    assert_eq!(
        caller_account_hash, known_account_hash,
        "caller account hash was not known account hash"
    );

    let entry_points = {
        let mut entry_points = EntryPoints::new();
        // takes no args, ret's PublicKey
        let entry_point = EntryPoint::new(
            ENTRY_POINT_NAME.to_string(),
            Vec::new(),
            CLType::ByteArray(32),
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_hash, _contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
        None,
    );

    let subcall_account_hash: AccountHash =
        runtime::call_contract(contract_hash, ENTRY_POINT_NAME, RuntimeArgs::default());
    assert_eq!(
        subcall_account_hash, known_account_hash,
        "subcall account hash was not known account hash"
    );
}
