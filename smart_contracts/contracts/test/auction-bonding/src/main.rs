#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use auction::{DelegationRate, METHOD_ADD_BID, METHOD_WITHDRAW_BID};
use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    account::AccountHash, auction, runtime_args, ApiError, ContractHash, PublicKey, RuntimeArgs,
    URef, U512,
};

const ARG_AMOUNT: &str = "amount";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_BOND: &str = "bond";
const ARG_UNBOND: &str = "unbond";
const ARG_ACCOUNT_HASH: &str = "account_hash";
const ARG_PUBLIC_KEY: &str = "public_key";
const TEST_BOND_FROM_MAIN_PURSE: &str = "bond-from-main-purse";
const TEST_SEED_NEW_ACCOUNT: &str = "seed_new_account";

#[repr(u16)]
enum Error {
    UnableToSeedAccount,
    UnknownCommand,
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT);

    match command.as_str() {
        ARG_BOND => bond(),
        ARG_UNBOND => unbond(),
        TEST_BOND_FROM_MAIN_PURSE => bond_from_main_purse(),
        TEST_SEED_NEW_ACCOUNT => seed_new_account(),
        _ => runtime::revert(ApiError::User(Error::UnknownCommand as u16)),
    }
}

fn bond() {
    let auction_contract_hash = system::get_auction();
    // Creates new purse with desired amount based on main purse and sends funds
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let bonding_purse = system::create_purse();

    system::transfer_from_purse_to_purse(account::get_main_purse(), bonding_purse, amount)
        .unwrap_or_revert();

    call_bond(auction_contract_hash, public_key, amount, bonding_purse);
}

fn bond_from_main_purse() {
    let auction_contract_hash = system::get_auction();
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    call_bond(
        auction_contract_hash,
        public_key,
        amount,
        account::get_main_purse(),
    );
}

fn call_bond(auction: ContractHash, public_key: PublicKey, bond_amount: U512, bonding_purse: URef) {
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_SOURCE_PURSE => bonding_purse,
        auction::ARG_DELEGATION_RATE => DelegationRate::from(42u8),
        auction::ARG_AMOUNT => bond_amount,
    };

    let (_purse, _amount): (URef, U512) = runtime::call_contract(auction, METHOD_ADD_BID, args);
}

fn unbond() {
    let auction_contract_hash = system::get_auction();
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let public_key: PublicKey = runtime::get_named_arg(ARG_PUBLIC_KEY);
    call_unbond(auction_contract_hash, public_key, amount);
}

fn call_unbond(auction: ContractHash, public_key: PublicKey, unbond_amount: U512) -> (URef, U512) {
    let args = runtime_args! {
        ARG_AMOUNT => unbond_amount,
        ARG_PUBLIC_KEY => public_key,
    };
    runtime::call_contract(auction, METHOD_WITHDRAW_BID, args)
}

fn seed_new_account() {
    let source = account::get_main_purse();
    let target: AccountHash = runtime::get_named_arg(ARG_ACCOUNT_HASH);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    system::transfer_from_purse_to_account(source, target, amount)
        .unwrap_or_revert_with(ApiError::User(Error::UnableToSeedAccount as u16));
}
