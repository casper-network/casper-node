#![no_std]
#![cfg_attr(not(test), no_main)]

#[no_mangle]
pub extern "C" fn bond() {
    pos::bond();
}

#[no_mangle]
pub extern "C" fn unbond() {
    pos::unbond();
}

#[no_mangle]
pub extern "C" fn get_payment_purse() {
    pos::get_payment_purse();
}

#[no_mangle]
pub extern "C" fn set_refund_purse() {
    pos::set_refund_purse();
}

#[no_mangle]
pub extern "C" fn get_refund_purse() {
    pos::get_refund_purse();
}

#[no_mangle]
pub extern "C" fn finalize_payment() {
    pos::finalize_payment();
}
