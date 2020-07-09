#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn call() {
    create_purse_01::delegate()
}
