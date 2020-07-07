#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use contract::contract_api::{runtime, system};
use types::{runtime_args, ApiError, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const BOND: &str = "bond";
const UNBOND: &str = "unbond";
const ARG_ENTRY_POINT_NAME: &str = "method";

fn bond(bond_amount: U512, bonding_purse: URef) {
    let contract_hash = system::get_proof_of_stake();
    let args = runtime_args! {
        ARG_AMOUNT => bond_amount,
        ARG_PURSE => bonding_purse,
    };
    runtime::call_contract(contract_hash, BOND, args)
}

fn unbond(unbond_amount: Option<U512>) {
    let contract_hash = system::get_proof_of_stake();
    let args = runtime_args! {
        ARG_AMOUNT => unbond_amount,
    };
    runtime::call_contract(contract_hash, UNBOND, args)
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT_NAME);
    match command.as_str() {
        BOND => {
            let rewards_purse: URef = runtime::get_named_arg(ARG_PURSE);
            let available_reward = runtime::get_named_arg(ARG_AMOUNT);
            // Attempt to bond using the rewards purse - should not be possible
            bond(available_reward, rewards_purse);
        }
        UNBOND => {
            unbond(None);
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    }
}
