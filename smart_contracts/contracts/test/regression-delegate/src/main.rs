#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, RuntimeArgs, U512};

const ARG_AMOUNT: &str = "amount";

const ARG_VALIDATOR: &str = "validator";
const ARG_DELEGATOR: &str = "delegator";

fn delegate(delegator: PublicKey, validator: PublicKey, amount: U512) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount + U512::one(),
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_DELEGATE, args);
}

#[no_mangle]
pub extern "C" fn call() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    delegate(delegator, validator, amount);
}
