#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{
    runtime_args,
    system::auction::{self, DelegationRate},
    ContractHash, PublicKey, RuntimeArgs, SecretKey, U512,
};

const DELEGATION_RATE: DelegationRate = 42;

fn bond(contract_hash: ContractHash, bond_amount: U512) {
    let valid_public_key: PublicKey =
        SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH])
            .unwrap()
            .into();

    let runtime_args = runtime_args! {
        auction::ARG_PUBLIC_KEY => valid_public_key,
        auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        auction::ARG_AMOUNT => bond_amount,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, runtime_args);
}

#[no_mangle]
pub extern "C" fn call() {
    // bond amount == 0 should fail
    bond(system::get_auction(), U512::from(0));
}
