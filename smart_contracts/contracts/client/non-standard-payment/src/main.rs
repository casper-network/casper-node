#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::{
    contract_api::{self, account, runtime, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    api_error,
    bytesrepr::{self, FromBytes},
    ApiError, RuntimeArgs, URef, U512,
};

const ARG_AMOUNT: &str = "amount";
const ARG_SOURCE_UREF: &str = "source";
const GET_PAYMENT_PURSE: &str = "get_payment_purse";

/// This logic is intended to be used as SESSION PAYMENT LOGIC
/// Alternate payment logic that allows payment from a purse other than the executing [Account]'s
/// main purse. A `Key::Uref` to the source purse must already exist in the executing context's
/// named keys under the name passed in as the `purse_name` argument.
#[no_mangle]
pub extern "C" fn call() {
    // source purse uref by name (from current context's named keys)
    let purse_uref = {
        match get_named_arg_if_exists(ARG_SOURCE_UREF) {
            Some(purse_uref) => purse_uref,
            None => account::get_main_purse(),
        }
    };

    // handle payment contract
    let handle_payment_contract_hash = system::get_handle_payment();

    // get payment purse for current execution
    let payment_purse: URef = runtime::call_contract(
        handle_payment_contract_hash,
        GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    // amount to transfer from named purse to payment purse
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(purse_uref, payment_purse, amount, None)
        .unwrap_or_revert();
}

fn get_named_arg_if_exists<T: FromBytes>(name: &str) -> Option<T> {
    let arg_size = runtime::get_named_arg_size(name)?;
    let arg_bytes = if arg_size > 0 {
        let res = {
            let data_non_null_ptr = contract_api::alloc_bytes(arg_size);
            let ret = unsafe {
                ext_ffi::casper_get_named_arg(
                    name.as_bytes().as_ptr(),
                    name.len(),
                    data_non_null_ptr.as_ptr(),
                    arg_size,
                )
            };
            let data =
                unsafe { Vec::from_raw_parts(data_non_null_ptr.as_ptr(), arg_size, arg_size) };
            api_error::result_from(ret).map(|_| data)
        };
        // Assumed to be safe as `get_named_arg_size` checks the argument already
        res.unwrap_or_revert()
    } else {
        // Avoids allocation with 0 bytes and a call to get_named_arg
        Vec::new()
    };
    let value = bytesrepr::deserialize(arg_bytes).unwrap_or_revert_with(ApiError::InvalidArgument);
    Some(value)
}
