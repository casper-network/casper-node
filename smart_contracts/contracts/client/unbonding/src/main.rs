#![no_std]
#![no_main]

extern crate alloc;

use contract::contract_api::{runtime, system};
use types::{runtime_args, RuntimeArgs, U512};

const UNBOND_METHOD_NAME: &str = "unbond";
const ARG_AMOUNT: &str = "amount";

// Unbonding contract.
//
// Accepts unbonding amount (of type `Option<u64>`) as first argument.
// Unbonding with `None` unbonds all stakes in the PoS contract.
// Otherwise (`Some<u64>`) unbonds with part of the bonded stakes.
#[no_mangle]
pub extern "C" fn call() {
    let unbond_amount: Option<U512> =
        runtime::get_named_arg::<Option<u64>>(ARG_AMOUNT).map(Into::into);

    let contract_hash = system::get_proof_of_stake();
    let args = runtime_args! {
        ARG_AMOUNT => unbond_amount,
    };
    runtime::call_contract(contract_hash, UNBOND_METHOD_NAME, args)
}
