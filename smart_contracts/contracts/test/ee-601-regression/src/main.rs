#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, Phase, RuntimeArgs, URef, U512};

const GET_PAYMENT_PURSE: &str = "get_payment_purse";
const NEW_UREF_RESULT_UREF_NAME: &str = "new_uref_result";
const ARG_AMOUNT: &str = "amount";

#[repr(u16)]
enum Error {
    InvalidPhase = 0,
}

#[no_mangle]
pub extern "C" fn call() {
    let phase = runtime::get_phase();
    if phase == Phase::Payment {
        let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

        let payment_purse: URef = runtime::call_contract(
            system::get_handle_payment(),
            GET_PAYMENT_PURSE,
            RuntimeArgs::default(),
        );

        system::transfer_from_purse_to_purse(account::get_main_purse(), payment_purse, amount, None)
            .unwrap_or_revert()
    }

    let value: Option<&str> = {
        match phase {
            Phase::Payment => Some("payment"),
            Phase::Session => Some("session"),
            _ => None,
        }
    };
    let value = value.unwrap_or_revert_with(ApiError::User(Error::InvalidPhase as u16));
    let result_key = storage::new_uref(value.to_string()).into();
    let mut uref_name: String = NEW_UREF_RESULT_UREF_NAME.to_string();
    uref_name.push('-');
    uref_name.push_str(value);
    runtime::put_key(&uref_name, result_key);
}
