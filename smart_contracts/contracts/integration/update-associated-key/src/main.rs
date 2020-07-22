#![no_std]
#![no_main]

use casperlabs_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::{AccountHash, Weight},
    ApiError,
};

#[repr(u16)]
enum Error {
    UpdateAssociatedKey = 100,
}

impl Into<ApiError> for Error {
    fn into(self) -> ApiError {
        ApiError::User(self as u16)
    }
}

const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";

#[no_mangle]
pub extern "C" fn call() {
    let account: AccountHash = runtime::get_named_arg(ARG_ACCOUNT);
    let weight_val: u32 = runtime::get_named_arg(ARG_WEIGHT);
    let weight = Weight::new(weight_val as u8);

    account::update_associated_key(account, weight)
        .unwrap_or_revert_with(Error::UpdateAssociatedKey);
}
