#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, string::ToString, vec};

use casperlabs_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys, Parameter},
    CLType, CLValue,
};
use standard_payment::ARG_AMOUNT;

const METHOD_CALL: &str = "call";
const HASH_KEY_NAME: &str = "standard_payment_hash";
const ACCESS_KEY_NAME: &str = "standard_payment_access";

#[no_mangle]
pub extern "C" fn call() {
    standard_payment::delegate();
}

#[no_mangle]
pub extern "C" fn install() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            METHOD_CALL.to_string(),
            vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
            CLType::Result {
                ok: Box::new(CLType::Unit),
                err: Box::new(CLType::U32),
            },
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let named_keys = NamedKeys::new();

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t(contract_key).unwrap_or_revert();
    runtime::ret(return_value);
}
