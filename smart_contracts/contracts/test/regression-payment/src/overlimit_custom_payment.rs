#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    bytesrepr::Bytes,
    system::{handle_payment, standard_payment},
    Phase, RuntimeArgs, URef, U512,
};

const ARG_ITERATIONS: &str = "iterations";
const SCRATCH_BYTES: usize = 4096;

#[no_mangle]
pub extern "C" fn call() {
    if runtime::get_phase() != Phase::Payment {
        return;
    }

    let iterations: u32 = runtime::get_named_arg(ARG_ITERATIONS);
    let scratch: Bytes = vec![0u8; SCRATCH_BYTES].into();
    let scratch_uref = storage::new_uref(scratch.clone());
    for _ in 0..iterations {
        storage::write(scratch_uref, scratch.clone());
    }

    let amount: U512 = runtime::get_named_arg(standard_payment::ARG_AMOUNT);
    let payment_purse: URef = runtime::call_contract(
        system::get_handle_payment(),
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );
    system::transfer_from_purse_to_purse(account::get_main_purse(), payment_purse, amount, None)
        .unwrap_or_revert();
}
