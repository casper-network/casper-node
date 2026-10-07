#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    system::{handle_payment, standard_payment},
    Phase, RuntimeArgs, URef, U512,
};

const SESSION_MARKER: &str = "split-custom-payment-session";

#[no_mangle]
pub extern "C" fn call() {
    match runtime::get_phase() {
        Phase::Payment => {
            let amount: U512 = runtime::get_named_arg(standard_payment::ARG_AMOUNT);
            let payment_purse: URef = runtime::call_contract(
                system::get_handle_payment(),
                handle_payment::METHOD_GET_PAYMENT_PURSE,
                RuntimeArgs::default(),
            );

            let first = amount / U512::from(2);
            let second = amount.saturating_sub(first);
            system::transfer_from_purse_to_purse(
                account::get_main_purse(),
                payment_purse,
                first,
                None,
            )
            .unwrap_or_revert();
            system::transfer_from_purse_to_purse(
                account::get_main_purse(),
                payment_purse,
                second,
                None,
            )
            .unwrap_or_revert();
        }
        Phase::Session => {
            runtime::put_key(
                SESSION_MARKER,
                storage::new_uref(SESSION_MARKER.to_string()).into(),
            );
        }
        _ => {}
    }
}
