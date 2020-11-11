#![no_std]
#![no_main]

extern crate alloc;

use auction::DelegationRate;
use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATION_RATE: &str = "delegation_rate";
const ARG_PUBLIC_KEY: &str = "public_key";

fn add_bid(
    public_key: PublicKey,
    bonding_purse: URef,
    bond_amount: U512,
    delegation_rate: DelegationRate,
) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_SOURCE_PURSE => bonding_purse,
        auction::ARG_AMOUNT => bond_amount,
        auction::ARG_DELEGATION_RATE => delegation_rate,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_BID, args);
}

// Bidding contract.
//
// Accepts a public key, amount and a delgation rate.
// Issues an add bid request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let bond_amount = runtime::get_named_arg(ARG_AMOUNT);
    let delegation_rate = runtime::get_named_arg(ARG_DELEGATION_RATE);

    // provision bidding purse
    let bidding_purse = {
        let source_purse = account::get_main_purse();
        let bonding_purse = system::create_purse();
        // transfer amount to a bidding purse
        system::transfer_from_purse_to_purse(source_purse, bonding_purse, bond_amount)
            .unwrap_or_revert();
        bonding_purse
    };

    add_bid(public_key, bidding_purse, bond_amount, delegation_rate);
}
