#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};
use casper_types::{AccessRights, ApiError, URef};

const ARG_CONTRACT_UREF: &str = "contract_uref";

#[repr(u16)]
enum Error {
    InvalidURefArg,
}

const REPLACEMENT_DATA: &str = "bawitdaba";

#[no_mangle]
pub extern "C" fn call() {
    let uref: URef = runtime::get_named_arg(ARG_CONTRACT_UREF);

    let is_valid = runtime::is_valid_uref(uref);
    if !is_valid {
        runtime::revert(ApiError::User(Error::InvalidURefArg as u16))
    }

    let forged_reference: URef = URef::new(uref.addr(), AccessRights::READ_ADD_WRITE);

    storage::write(forged_reference, REPLACEMENT_DATA)
}
