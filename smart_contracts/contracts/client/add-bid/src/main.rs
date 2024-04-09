#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{
    runtime_args,
    system::auction::{
        self, DelegationRate, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_INACTIVE_VALIDATOR_UNDELEGATION_DELAY, ARG_PUBLIC_KEY,
    },
    PublicKey, U512,
};

fn add_bid(
    public_key: PublicKey,
    bond_amount: U512,
    delegation_rate: DelegationRate,
    inactive_validator_undelegation_delay: Option<u64>,
) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        ARG_PUBLIC_KEY => public_key,
        ARG_AMOUNT => bond_amount,
        ARG_DELEGATION_RATE => delegation_rate,
        ARG_INACTIVE_VALIDATOR_UNDELEGATION_DELAY => inactive_validator_undelegation_delay,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, args);
}

// Bidding contract.
//
// Accepts a public key, amount and a delegation rate.
// Issues an add bid request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let bond_amount = runtime::get_named_arg(ARG_AMOUNT);
    let delegation_rate = runtime::get_named_arg(ARG_DELEGATION_RATE);
    let inactive_validator_undelegation_delay =
        runtime::get_named_arg(ARG_INACTIVE_VALIDATOR_UNDELEGATION_DELAY);

    add_bid(
        public_key,
        bond_amount,
        delegation_rate,
        inactive_validator_undelegation_delay,
    );
}
