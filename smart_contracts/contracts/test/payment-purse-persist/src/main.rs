#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
};
use casper_contract::{
    contract_api::{account, runtime, runtime::put_key, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{ContractHash, ContractPackageHash},
    runtime_args, AddressableEntityHash, ApiError, CLType, CLValue, EntityEntryPoint,
    EntryPointAccess, EntryPointPayment, EntryPointType, EntryPoints, Key, RuntimeArgs, URef, U512,
};

const GET_PAYMENT_PURSE: &str = "get_payment_purse";
const THIS_SHOULD_FAIL: &str = "this_should_fail";
const HELPER_HASH_KEY: &str = "payment_purse_persist_helper_hash";
const LEAK_PAYMENT_PURSE: &str = "leak_payment_purse";

const ARG_AMOUNT: &str = "amount";
const ARG_METHOD: &str = "method";

#[no_mangle]
pub extern "C" fn leak_payment_purse() {
    let handle_payment_contract_hash = system::get_handle_payment();
    let payment_purse: URef = runtime::call_contract(
        handle_payment_contract_hash,
        GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    runtime::ret(CLValue::from_t(payment_purse).unwrap_or_revert())
}

fn install_helper() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntityEntryPoint::new(
        LEAK_PAYMENT_PURSE.to_string(),
        vec![],
        CLType::URef,
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    ));

    let (contract_hash, _) = storage::new_contract(entry_points, None, None, None, None);
    runtime::put_key(
        HELPER_HASH_KEY,
        Key::contract_entity_key(AddressableEntityHash::new(contract_hash.value())),
    );
}

/// This logic is intended to be used as SESSION PAYMENT LOGIC
/// It gets the payment purse and attempts and attempts to persist it,
/// which should fail.
#[no_mangle]
pub extern "C" fn call() {
    let method: String = runtime::get_named_arg(ARG_METHOD);

    if method == "install_helper" {
        install_helper();
        return;
    }

    if method == "put_key" {
        // handle payment contract
        let handle_payment_contract_hash = system::get_handle_payment();

        // get payment purse for current execution
        let payment_purse: URef = runtime::call_contract(
            handle_payment_contract_hash,
            GET_PAYMENT_PURSE,
            RuntimeArgs::default(),
        );

        // attempt to persist the payment purse, which should fail
        put_key(THIS_SHOULD_FAIL, payment_purse.into());
    } else if method == "subcall_put_key" {
        let helper_hash_key = runtime::get_key(HELPER_HASH_KEY).unwrap_or_revert();
        let helper_hash = helper_hash_key
            .into_entity_hash_addr()
            .map(ContractHash::new)
            .unwrap_or_revert();
        let payment_purse: URef =
            runtime::call_contract(helper_hash, LEAK_PAYMENT_PURSE, RuntimeArgs::default());

        // attempt to persist the payment purse returned by a stored-contract subcall
        put_key(THIS_SHOULD_FAIL, payment_purse.into());

        let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
        system::transfer_from_purse_to_purse(
            account::get_main_purse(),
            payment_purse,
            amount,
            None,
        )
        .unwrap_or_revert();
    } else if method == "call_contract" {
        // handle payment contract
        let handle_payment_contract_hash = system::get_handle_payment();

        // get payment purse for current execution
        let payment_purse: URef = runtime::call_contract(
            handle_payment_contract_hash,
            GET_PAYMENT_PURSE,
            RuntimeArgs::default(),
        );

        // attempt to call a contract with the payment purse, which should fail
        let _payment_purse: URef = runtime::call_contract(
            handle_payment_contract_hash,
            GET_PAYMENT_PURSE,
            runtime_args! {
                "payment_purse" => payment_purse,
            },
        );

        // should never reach here
        runtime::revert(ApiError::User(1000));
    } else if method == "call_versioned_contract" {
        // handle payment contract
        let handle_payment_contract_hash = system::get_handle_payment();

        // get payment purse for current execution
        let payment_purse: URef = runtime::call_contract(
            handle_payment_contract_hash,
            GET_PAYMENT_PURSE,
            RuntimeArgs::default(),
        );

        // attempt to call a versioned contract with the payment purse, which should fail
        let _payment_purse: URef = runtime::call_versioned_contract(
            ContractPackageHash::new(handle_payment_contract_hash.value()),
            None, // Latest
            GET_PAYMENT_PURSE,
            runtime_args! {
                "payment_purse" => payment_purse,
            },
        );

        // should never reach here
        runtime::revert(ApiError::User(1001));
    } else {
        runtime::revert(ApiError::User(2000));
    }
}
