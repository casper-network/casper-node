#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{runtime_args, ApiError, RuntimeArgs, URef, U512};

const GET_PAYMENT_PURSE: &str = "get_payment_purse";
const SET_REFUND_PURSE: &str = "set_refund_purse";

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const ARG_PURSE_NAME: &str = "purse_name";

/// This logic is intended to be used as SESSION PAYMENT LOGIC
/// Alternate payment logic that allows payment from a purse other than the executing [Account]'s
/// main purse. A `Key::Uref` to the source purse must already exist in the executing context's
/// named keys under the name passed in as the `purse_name` argument.
#[no_mangle]
pub extern "C" fn call() {
    // source purse uref by name (from current context's named keys)
    let purse_uref = {
        let purse_name: String = runtime::get_named_arg(ARG_PURSE_NAME);
        runtime::get_key(&purse_name)
            .unwrap_or_revert_with(ApiError::InvalidPurseName)
            .into_uref()
            .unwrap_or_revert_with(ApiError::InvalidPurse)
    };

    // amount to transfer from named purse to payment purse
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    // proof of stake contract
    let pos_contract_hash = system::get_proof_of_stake();

    // set refund purse to source purse
    {
        let contract_hash = pos_contract_hash;
        let args = runtime_args! {
            ARG_PURSE => purse_uref,
        };
        runtime::call_contract::<()>(contract_hash, SET_REFUND_PURSE, args);
    }

    // get payment purse for current execution
    let payment_purse: URef =
        runtime::call_contract(pos_contract_hash, GET_PAYMENT_PURSE, RuntimeArgs::default());

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(purse_uref, payment_purse, amount).unwrap_or_revert();
}
