#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, ApiError, RuntimeArgs, URef, U512};

const GET_PAYMENT_PURSE: &str = "get_payment_purse";
const SET_REFUND_PURSE: &str = "set_refund_purse";

#[repr(u16)]
enum Error {
    PosNotFound = 1,
    NamedPurseNotFound = 2,
}

impl Into<ApiError> for Error {
    fn into(self) -> ApiError {
        ApiError::User(self as u16)
    }
}

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const ARG_PURSE_NAME: &str = "purse_name";

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let name: String = runtime::get_named_arg(ARG_PURSE_NAME);

    // get uref from current context's named_keys
    let source = runtime::get_key(&name)
        .unwrap_or_revert_with(Error::NamedPurseNotFound)
        .into_uref()
        .unwrap_or_revert_with(Error::PosNotFound);

    let pos_contract_hash = system::get_proof_of_stake();

    // set refund purse to source purse
    {
        let contract_hash = pos_contract_hash;
        let runtime_args = runtime_args! {
            ARG_PURSE => source,
        };
        runtime::call_contract::<()>(contract_hash, SET_REFUND_PURSE, runtime_args);
    }

    // fund payment purse
    {
        let target: URef =
            runtime::call_contract(pos_contract_hash, GET_PAYMENT_PURSE, RuntimeArgs::default());

        system::transfer_from_purse_to_purse(source, target, amount).unwrap_or_revert();
    }
}
