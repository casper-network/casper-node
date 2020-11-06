#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::contract_api::{account, storage};

#[no_mangle]
pub extern "C" fn call() {
    let uref = storage::new_uref(());
    loop {
        let _ = account::get_main_purse();
        storage::write(uref, vec![0u8; 4096]);
    }
}
