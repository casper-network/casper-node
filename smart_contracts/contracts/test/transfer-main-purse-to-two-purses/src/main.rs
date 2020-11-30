#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, URef, U512};

const DESTINATION_PURSE_ONE: &str = "destination_purse_one";
const DESTINATION_PURSE_TWO: &str = "destination_purse_two";
const TRANSFER_AMOUNT_ONE: &str = "transfer_amount_one";
const TRANSFER_AMOUNT_TWO: &str = "transfer_amount_two";

#[repr(u16)]
enum CustomError {
    TransferToPurseOneFailed = 101,
    TransferToPurseTwoFailed = 102,
}

fn get_or_create_purse(purse_name: &str) -> URef {
    match runtime::get_key(purse_name) {
        None => {
            // Create and store purse if doesn't exist
            let purse = system::create_purse();
            runtime::put_key(purse_name, purse.into());
            purse
        }
        Some(purse_key) => match purse_key.as_uref() {
            Some(uref) => *uref,
            None => runtime::revert(ApiError::UnexpectedKeyVariant),
        },
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let main_purse: URef = account::get_main_purse();

    let destination_purse_one_name: String = runtime::get_named_arg(DESTINATION_PURSE_ONE);

    let destination_purse_one = get_or_create_purse(&destination_purse_one_name);

    let destination_purse_two_name: String = runtime::get_named_arg(DESTINATION_PURSE_TWO);
    let transfer_amount_one: U512 = runtime::get_named_arg(TRANSFER_AMOUNT_ONE);

    let destination_purse_two = get_or_create_purse(&destination_purse_two_name);

    let transfer_amount_two: U512 = runtime::get_named_arg(TRANSFER_AMOUNT_TWO);

    system::transfer_from_purse_to_purse(
        main_purse,
        destination_purse_one,
        transfer_amount_one,
        None,
    )
    .unwrap_or_revert_with(ApiError::User(CustomError::TransferToPurseOneFailed as u16));
    system::transfer_from_purse_to_purse(
        main_purse,
        destination_purse_two,
        transfer_amount_two,
        None,
    )
    .unwrap_or_revert_with(ApiError::User(CustomError::TransferToPurseTwoFailed as u16));
}
