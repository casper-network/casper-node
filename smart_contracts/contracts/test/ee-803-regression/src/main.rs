#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use auction::DelegationRate;
use casper_contract::contract_api::{runtime, system};
use casper_types::{auction, runtime_args, ApiError, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";
const ADD_BID: &str = "add_bid";
const WITHDRAW_BID: &str = "withdraw_bid";
const ARG_ENTRY_POINT_NAME: &str = "method";
const TEST_PUBLIC_KEY: PublicKey = PublicKey::Ed25519([42; 32]);

fn add_bid(bond_amount: U512, bonding_purse: URef) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => TEST_PUBLIC_KEY,
        auction::ARG_SOURCE_PURSE => bonding_purse,
        auction::ARG_DELEGATION_RATE => DelegationRate::from(42u8),
        auction::ARG_AMOUNT => bond_amount,
    };
    runtime::call_contract(contract_hash, ADD_BID, args)
}

fn withdraw_bid(unbond_amount: Option<U512>) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => TEST_PUBLIC_KEY,
        ARG_AMOUNT => unbond_amount,
    };
    runtime::call_contract(contract_hash, WITHDRAW_BID, args)
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT_NAME);
    match command.as_str() {
        ADD_BID => {
            let rewards_purse: URef = runtime::get_named_arg(ARG_PURSE);
            let available_reward = runtime::get_named_arg(ARG_AMOUNT);
            // Attempt to bond using the rewards purse - should not be possible
            add_bid(available_reward, rewards_purse);
        }
        WITHDRAW_BID => {
            withdraw_bid(None);
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    }
}
