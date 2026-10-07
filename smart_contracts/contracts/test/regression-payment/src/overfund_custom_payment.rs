#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    system::{handle_payment, standard_payment},
    Phase, RuntimeArgs, URef, U512,
};

const ARG_EXTRA: &str = "extra";
const ARG_SOURCE: &str = "source";
const SESSION_MARKER: &str = "overfund-custom-payment-session";

#[no_mangle]
pub extern "C" fn call() {
    match runtime::get_phase() {
        Phase::Payment => {
            let amount: U512 = runtime::get_named_arg(standard_payment::ARG_AMOUNT);
            let extra: U512 = runtime::get_named_arg(ARG_EXTRA);
            let source_name: String = runtime::get_named_arg(ARG_SOURCE);
            let source_purse = runtime::get_key(&source_name)
                .and_then(|key| key.into_uref())
                .unwrap_or_revert();
            let payment_purse: URef = runtime::call_contract(
                system::get_handle_payment(),
                handle_payment::METHOD_GET_PAYMENT_PURSE,
                RuntimeArgs::default(),
            );

            system::transfer_from_purse_to_purse(source_purse, payment_purse, amount + extra, None)
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
