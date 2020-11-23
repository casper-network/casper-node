#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::U512;

const ARG_TOTAL_PURSES: &str = "total_purses";
const ARG_SEED_AMOUNT: &str = "seed_amount";

#[no_mangle]
pub extern "C" fn call() {
    let total_purses: u64 = runtime::get_named_arg(ARG_TOTAL_PURSES);
    let seed_amount: U512 = runtime::get_named_arg(ARG_SEED_AMOUNT);

    for i in 0..total_purses {
        let new_purse = system::create_purse();
        system::transfer_from_purse_to_purse(
            account::get_main_purse(),
            new_purse,
            seed_amount,
            None,
        )
        .unwrap_or_revert();

        let name = format!("purse:{}", i);
        runtime::put_key(&name, new_purse.into());
    }
}
