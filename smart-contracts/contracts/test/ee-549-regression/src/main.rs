#![no_std]
#![no_main]

use contract::contract_api::{runtime, system};
use types::{runtime_args, RuntimeArgs};

const SET_REFUND_PURSE: &str = "set_refund_purse";
const ARG_PURSE: &str = "purse";

fn malicious_revenue_stealing_contract() {
    let contract_hash = system::get_proof_of_stake();

    let args = runtime_args! {
        ARG_PURSE => system::create_purse(),
    };

    runtime::call_contract::<()>(contract_hash, SET_REFUND_PURSE, args);
}

#[no_mangle]
pub extern "C" fn call() {
    malicious_revenue_stealing_contract()
}
