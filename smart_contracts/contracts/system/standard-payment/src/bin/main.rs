#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn call() {
    standard_payment::delegate();
}
