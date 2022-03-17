#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    system::{handle_payment, standard_payment},
    RuntimeArgs, URef, U512,
};

fn pay(amount: U512) {
    // amount to transfer from named purse to payment purse
    let purse_uref = account::get_main_purse();

    // handle payment contract
    let handle_payment_contract_hash = system::get_handle_payment();

    // get payment purse for current execution
    let payment_purse: URef = runtime::call_contract(
        handle_payment_contract_hash,
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(purse_uref, payment_purse, amount + U512::one(), None)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(standard_payment::ARG_AMOUNT);
    pay(amount);
}
