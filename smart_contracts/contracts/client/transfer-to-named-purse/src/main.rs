#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, Key, U512};

const ARG_PURSE_NAME: &str = "purse_name";
const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn call() {
    let purse_name: String = runtime::get_named_arg(ARG_PURSE_NAME);

    let purse_uref = match runtime::get_key(&purse_name) {
        Some(Key::URef(uref)) => uref,
        Some(_) => {
            // Found a key but it is not a purse
            runtime::revert(ApiError::UnexpectedKeyVariant);
        }
        None => {
            // Creates new named purse
            let new_purse = system::create_purse();
            runtime::put_key(&purse_name, new_purse.into());
            new_purse
        }
    };

    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let source_purse = account::get_main_purse();

    system::transfer_from_purse_to_purse(source_purse, purse_uref, amount, None).unwrap_or_revert();
}
