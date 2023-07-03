#![no_std]
#![no_main]

#[allow(unused)]
use casper_contract::contract_api::runtime;

#[inline(never)]
fn layer_4() {
    panic!("Simulating stack height limit")
}

#[inline(never)]
fn layer_3() {
    layer_4()
}

#[inline(never)]
fn layer_2() {
    layer_3()
}

#[inline(never)]
fn layer_1() {
    layer_2()
}

#[no_mangle]
pub extern "C" fn call() {
    layer_1();
}
