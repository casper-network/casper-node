#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::system::{AUCTION, HANDLE_PAYMENT, MINT};

#[no_mangle]
pub extern "C" fn call() {
    runtime::put_key(MINT, system::get_mint().into());
    runtime::put_key(HANDLE_PAYMENT, system::get_handle_payment().into());
    runtime::put_key(AUCTION, system::get_auction().into());
}
