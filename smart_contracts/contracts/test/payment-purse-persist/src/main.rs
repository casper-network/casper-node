#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::contract_api::{runtime, runtime::put_key, system};
use casper_types::{contracts::ContractPackageHash, runtime_args, ApiError, RuntimeArgs, URef};

const GET_PAYMENT_PURSE: &str = "get_payment_purse";
const THIS_SHOULD_FAIL: &str = "this_should_fail";

const ARG_METHOD: &str = "method";

/// This logic is intended to be used as SESSION PAYMENT LOGIC
/// It gets the payment purse and attempts and attempts to persist it,
/// which should fail.
#[no_mangle]
pub extern "C" fn call() {
    let method: String = runtime::get_named_arg(ARG_METHOD);

    // handle payment contract
    let handle_payment_contract_hash = system::get_handle_payment();

    // get payment purse for current execution
    let payment_purse: URef = runtime::call_contract(
        handle_payment_contract_hash,
        GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    if method == "put_key" {
        // attempt to persist the payment purse, which should fail
        put_key(THIS_SHOULD_FAIL, payment_purse.into());
    } else if method == "call_contract" {
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
