#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::{RuntimeArgs, URef};

const ENTRY_POINT_GET_PAYMENT_PURSE: &str = "get_payment_purse";

#[no_mangle]
pub extern "C" fn call() {
    let handle_payment = system::get_handle_payment();

    let _: URef = runtime::call_contract(
        handle_payment,
        ENTRY_POINT_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    let _ = system::create_purse();
}
