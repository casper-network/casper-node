#![no_std]
#![no_main]

use contract::contract_api::account;

#[no_mangle]
pub extern "C" fn call() {
    loop {
        let _main_purse = account::get_main_purse();
    }
}
