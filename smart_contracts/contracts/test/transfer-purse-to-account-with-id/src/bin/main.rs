#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn call() {
    transfer_purse_to_account_with_id::delegate();
}
