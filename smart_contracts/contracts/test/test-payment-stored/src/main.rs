#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter},
    mint::ARG_AMOUNT,
    CLType, RuntimeArgs, URef, U512,
};

const ENTRY_FUNCTION_NAME: &str = "pay";
const HASH_KEY_NAME: &str = "test_payment_hash";
const PACKAGE_HASH_KEY_NAME: &str = "test_payment_package_hash";
const ACCESS_KEY_NAME: &str = "test_payment_access";
const ARG_NAME: &str = "amount";
const CONTRACT_VERSION: &str = "contract_version";
const GET_PAYMENT_PURSE: &str = "get_payment_purse";

#[no_mangle]
pub extern "C" fn pay() {
    // amount to transfer from named purse to payment purse
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let purse_uref = account::get_main_purse();

    // proof of stake contract
    let pos_contract_hash = system::get_proof_of_stake();

    // get payment purse for current execution
    let payment_purse: URef =
        runtime::call_contract(pos_contract_hash, GET_PAYMENT_PURSE, RuntimeArgs::default());

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(purse_uref, payment_purse, amount, None)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![Parameter::new(ARG_NAME, CLType::U512)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };
    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, contract_hash.into());
}
