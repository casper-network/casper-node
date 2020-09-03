#![no_std]
#![no_main]

use casper_contract::contract_api::{account, runtime};
use casper_types::URef;

const ARG_PURSE: &str = "purse";

#[no_mangle]
pub extern "C" fn call() {
    let known_main_purse: URef = runtime::get_named_arg(ARG_PURSE);
    let main_purse: URef = account::get_main_purse();
    assert_eq!(
        main_purse, known_main_purse,
        "main purse was not known purse"
    );
}
