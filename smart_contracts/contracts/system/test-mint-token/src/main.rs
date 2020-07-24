#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_types::{runtime_args, ContractHash, RuntimeArgs, URef, U512};

use casperlabs_contract::contract_api::{runtime, system};

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const ARG_SOURCE: &str = "source";
const ARG_TARGET: &str = "target";
const ARG_CREATE: &str = "create";
const ARG_TRANSFER: &str = "transfer";
const ARG_BALANCE: &str = "balance";

#[no_mangle]
pub extern "C" fn call() {
    let mint: ContractHash = system::get_mint();

    let source = get_purse(mint, 100);
    let target = get_purse(mint, 300);

    assert!(
        transfer(mint, source, target, U512::from(70)) == "Success!",
        "transfer should succeed"
    );

    assert!(
        balance(mint, source).unwrap() == U512::from(30),
        "source purse balance incorrect"
    );
    assert!(
        balance(mint, target).unwrap() == U512::from(370),
        "target balance incorrect"
    );
}

fn get_purse(mint: ContractHash, amount: u64) -> URef {
    let amount = U512::from(amount);
    let args = runtime_args! {
            ARG_AMOUNT => amount,
    };
    runtime::call_contract::<URef>(mint, ARG_CREATE, args)
}

fn transfer(mint: ContractHash, source: URef, target: URef, amount: U512) -> String {
    let args = runtime_args! {
        ARG_AMOUNT => amount,
        ARG_SOURCE => source,
        ARG_TARGET => target,
    };
    runtime::call_contract::<String>(mint, ARG_TRANSFER, args)
}

fn balance(mint: ContractHash, purse: URef) -> Option<U512> {
    let args = runtime_args! {
        ARG_PURSE => purse,
    };
    runtime::call_contract::<Option<U512>>(mint, ARG_BALANCE, args)
}
