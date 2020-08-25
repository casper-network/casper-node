#![no_std]
#![no_main]

extern crate alloc;

use casperlabs_contract::contract_api::{account, runtime, system};
use casperlabs_types::{runtime_args, ContractHash, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const BOND: &str = "bond";

fn bond(contract_hash: ContractHash, bond_amount: U512, bonding_purse: URef) {
    let runtime_args = runtime_args! {
        ARG_AMOUNT => bond_amount,
        ARG_PURSE => bonding_purse,
    };
    runtime::call_contract::<()>(contract_hash, BOND, runtime_args);
}

#[no_mangle]
pub extern "C" fn call() {
    // bond amount == 0 should fail
    bond(
        system::get_auction(),
        U512::from(0),
        account::get_main_purse(),
    );
}
