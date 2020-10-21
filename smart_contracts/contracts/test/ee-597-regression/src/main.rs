#![no_std]
#![no_main]

extern crate alloc;

use auction::DelegationRate;
use casper_contract::contract_api::{account, runtime, system};
use casper_types::{auction, runtime_args, ContractHash, PublicKey, RuntimeArgs, URef, U512};

const VALID_PUBLIC_KEY: PublicKey = PublicKey::Ed25519([42; 32]);

fn bond(contract_hash: ContractHash, bond_amount: U512, bonding_purse: URef) {
    let runtime_args = runtime_args! {
        auction::ARG_PUBLIC_KEY => VALID_PUBLIC_KEY,
        auction::ARG_SOURCE_PURSE => bonding_purse,
        auction::ARG_DELEGATION_RATE => DelegationRate::from(42u8),
        auction::ARG_AMOUNT => bond_amount,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, runtime_args);
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
