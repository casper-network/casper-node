#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::contract_api::{account, storage};
use casper_types::bytesrepr::Bytes;

#[no_mangle]
pub extern "C" fn call() {
    let uref = storage::new_uref(());
    loop {
        let _ = account::get_main_purse();
        let data: Bytes = vec![0u8; 4096].into();
        storage::write(uref, data);
    }
}
