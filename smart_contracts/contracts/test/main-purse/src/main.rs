#![no_std]
#![no_main]

use casper_contract::contract_api::{account, runtime};
use casper_types::{AccessRights, ApiError, URef};

const ARG_PURSE: &str = "purse";

#[no_mangle]
pub extern "C" fn call() {
    let known_main_purse: URef = runtime::get_named_arg(ARG_PURSE);
    let main_purse: URef = account::get_main_purse();
    if known_main_purse.is_writeable() {
        runtime::revert(ApiError::User(1))
    }
    if main_purse.with_access_rights(AccessRights::READ_ADD) != known_main_purse {
        runtime::revert(ApiError::User(2));
    }
}
