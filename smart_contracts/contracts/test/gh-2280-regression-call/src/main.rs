#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;

use casper_types::{account::AccountHash, runtime_args, ContractHash, RuntimeArgs};

const FAUCET_NAME: &str = "faucet";
const ARG_TARGET: &str = "target";
const ARG_CONTRACT_HASH: &str = "contract_hash";

fn call_faucet(contract_hash: ContractHash, target: AccountHash) {
    let faucet_args = runtime_args! {
        ARG_TARGET => target,
    };
    runtime::call_contract(contract_hash, FAUCET_NAME, faucet_args)
}

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
    let target: AccountHash = runtime::get_named_arg(ARG_TARGET);

    call_faucet(contract_hash, target);
}
