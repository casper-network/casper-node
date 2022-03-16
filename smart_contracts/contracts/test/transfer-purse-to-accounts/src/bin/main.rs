#![no_std]
#![no_main]

use casper_contract::contract_api::account;

#[no_mangle]
pub extern "C" fn call() {
    let source = account::get_main_purse();
    transfer_purse_to_accounts::delegate(source);
}
