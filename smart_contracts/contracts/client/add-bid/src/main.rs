#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::{
    contract_api,
    contract_api::{runtime, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    api_error, bytesrepr, bytesrepr::FromBytes, runtime_args, system::auction, ApiError, PublicKey,
    U512,
};

fn get_named_arg_size(name: &str) -> Option<usize> {
    let mut arg_size: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_named_arg_size(
            name.as_bytes().as_ptr(),
            name.len(),
            &mut arg_size as *mut usize,
        )
    };
    match api_error::result_from(ret) {
        Ok(_) => Some(arg_size),
        Err(ApiError::MissingArgument) => None,
        Err(e) => runtime::revert(e),
    }
}

// The optional here is literal and does not co-relate to an Option enum type.
// If the argument has been provided it is accepted, and is then turned into a Some.
// If the argument is not provided at all, then it is considered as None.
pub fn get_optional_named_args<T: FromBytes>(name: &str) -> Option<T> {
    let arg_size = get_named_arg_size(name)?;
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

    bytesrepr::deserialize(arg_bytes).ok()
}

fn add_bid(
    public_key: PublicKey,
    bond_amount: U512,
    delegation_rate: auction::DelegationRate,
    minimum_delegation_amount: Option<u64>,
    maximum_delegation_amount: Option<u64>,
    reserved_slots: Option<u32>,
) {
    let contract_hash = system::get_auction();
    let mut args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => bond_amount,
        auction::ARG_DELEGATION_RATE => delegation_rate,
    };
    // Optional arguments
    if let Some(minimum_delegation_amount) = minimum_delegation_amount {
        let _ = args.insert(
            auction::ARG_MINIMUM_DELEGATION_AMOUNT,
            minimum_delegation_amount,
        );
    }
    if let Some(maximum_delegation_amount) = maximum_delegation_amount {
        let _ = args.insert(
            auction::ARG_MAXIMUM_DELEGATION_AMOUNT,
            maximum_delegation_amount,
        );
    }
    if let Some(reserved_slots) = reserved_slots {
        let _ = args.insert(auction::ARG_RESERVED_SLOTS, reserved_slots);
    }
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, args);
}

// Bidding contract.
//
// Accepts a public key, amount and a delegation rate.
// Issues an add bid request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let bond_amount = runtime::get_named_arg(auction::ARG_AMOUNT);
    let delegation_rate = runtime::get_named_arg(auction::ARG_DELEGATION_RATE);

    // Optional arguments
    let minimum_delegation_amount = get_optional_named_args(auction::ARG_MINIMUM_DELEGATION_AMOUNT);
    let maximum_delegation_amount = get_optional_named_args(auction::ARG_MAXIMUM_DELEGATION_AMOUNT);
    let reserved_slots = get_optional_named_args(auction::ARG_RESERVED_SLOTS);

    add_bid(
        public_key,
        bond_amount,
        delegation_rate,
        minimum_delegation_amount,
        maximum_delegation_amount,
        reserved_slots,
    );
}
