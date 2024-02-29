#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};

fn add_bid(
    public_key: PublicKey,
    bond_amount: U512,
    delegation_rate: auction::DelegationRate,
    minimum_delegation_amount: Option<u64>,
    maximum_delegation_amount: Option<u64>,
) {
    let contract_hash = system::get_auction();
    let mut args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => bond_amount,
        auction::ARG_DELEGATION_RATE => delegation_rate,
    };
    // Optional arguments
    if let Some(minimum_delegation_amount) = minimum_delegation_amount {
        args.insert(
            auction::ARG_MINIMUM_DELEGATION_AMOUNT,
            minimum_delegation_amount,
        );
    }
    if let Some(maximum_delegation_amount) = maximum_delegation_amount {
        args.insert(
            auction::ARG_MAXIMUM_DELEGATION_AMOUNT,
            maximum_delegation_amount,
        );
    }
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, args);
}

// Bidding contract.
//
// Accepts a public key, amount and a delegation rate.
// Issues an add bid request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let bond_amount = runtime::get_named_arg(auction::ARG_AMOUNT);
    let delegation_rate = runtime::get_named_arg(auction::ARG_DELEGATION_RATE);
    // Optional arguments
    let minimum_delegation_amount =
        runtime::try_get_named_arg(auction::ARG_MINIMUM_DELEGATION_AMOUNT);
    let maximum_delegation_amount =
        runtime::try_get_named_arg(auction::ARG_MAXIMUM_DELEGATION_AMOUNT);

    add_bid(
        public_key,
        bond_amount,
        delegation_rate,
        minimum_delegation_amount,
        maximum_delegation_amount,
    );
}
