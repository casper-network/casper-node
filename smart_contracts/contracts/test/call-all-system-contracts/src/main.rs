#![no_std]
#![no_main]

use casper_contract::contract_api::{account, runtime, system};
use casper_types::{runtime_args, system::mint, U512};

#[no_mangle]
pub extern "C" fn call() {
    let mint = system::get_mint();
    let main_purse = account::get_main_purse();

    let _: Option<U512> = runtime::call_contract(
        mint,
        mint::METHOD_BALANCE,
        runtime_args! { mint::ARG_PURSE => main_purse },
    );

    let _ = system::create_purse();
}
