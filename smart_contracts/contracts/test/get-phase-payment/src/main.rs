#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{system::handle_payment, Phase, RuntimeArgs, URef, U512};

const ARG_PHASE: &str = "phase";
const ARG_AMOUNT: &str = "amount";

fn standard_payment(amount: U512) {
    let main_purse = account::get_main_purse();

    let handle_payment_pointer = system::get_handle_payment();

    let payment_purse: URef = runtime::call_contract(
        handle_payment_pointer,
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    system::transfer_from_purse_to_purse(main_purse, payment_purse, amount, None).unwrap_or_revert()
}

#[no_mangle]
pub extern "C" fn call() {
    let known_phase: Phase = runtime::get_named_arg(ARG_PHASE);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let get_phase = runtime::get_phase();
    assert_eq!(
        get_phase, known_phase,
        "get_phase did not return known_phase"
    );

    standard_payment(amount);
}
