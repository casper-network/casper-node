#![no_std]
#![no_main]

use casperlabs_contract::contract_api::{account, runtime, system};
use casperlabs_types::{runtime_args, ContractHash, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const BOND: &str = "bond";
const UNBOND: &str = "unbond";

fn bond(contract_hash: ContractHash, bond_amount: U512, bonding_purse: URef) {
    let runtime_args = runtime_args! {
        ARG_AMOUNT => bond_amount,
        ARG_PURSE => bonding_purse,
    };
    runtime::call_contract::<()>(contract_hash, BOND, runtime_args);
}

fn unbond(contract_hash: ContractHash, unbond_amount: Option<U512>) {
    let args = runtime_args! {
        ARG_AMOUNT => unbond_amount,
    };
    runtime::call_contract::<()>(contract_hash, UNBOND, args);
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let contract_hash = system::get_proof_of_stake();
    bond(contract_hash, amount, account::get_main_purse());
    unbond(contract_hash, Some(amount + 1));
}
