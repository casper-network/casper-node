#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{account, runtime, storage};
use casper_types::Key;

#[no_mangle]
pub extern "C" fn call() {
    let mut data: u32 = 1;
    let uref = storage::new_uref(data);
    runtime::put_key("new_key", Key::from(uref));
    loop {
        let _ = account::get_main_purse();
        data += 1;
        storage::write(uref, data);
    }
}
