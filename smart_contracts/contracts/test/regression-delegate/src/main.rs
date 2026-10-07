#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};

const ARG_AMOUNT: &str = "amount";

const ARG_VALIDATOR: &str = "validator";
const ARG_DELEGATOR: &str = "delegator";
const ARG_CALL_COUNT: &str = "call_count";

fn delegate(delegator: PublicKey, validator: PublicKey, amount: U512, over_limit: bool) {
    let contract_hash = system::get_auction();
    let delegate_amount = if over_limit {
        amount + U512::one()
    } else {
        amount
    };
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => delegate_amount,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_DELEGATE, args);
}

#[no_mangle]
pub extern "C" fn call() {
    let delegator: PublicKey = runtime::get_named_arg(ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    if let Some(call_count) = runtime::try_get_named_arg::<u32>(ARG_CALL_COUNT) {
        for _ in 0..call_count {
            delegate(delegator.clone(), validator.clone(), amount, false);
        }
    } else {
        delegate(delegator, validator, amount, true);
    }
}
