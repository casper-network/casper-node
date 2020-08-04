#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;

use casperlabs_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    contracts::NamedKeys,
    mint::{
        BidPurses, FounderPurses, Mint, UnbondingPurses, BID_PURSES_KEY, FOUNDER_PURSES_KEY,
        UNBONDING_PURSES_KEY,
    },
    CLValue, U512,
};
use mint_token::MintContract;

const HASH_KEY_NAME: &str = "mint_hash";
const ACCESS_KEY_NAME: &str = "mint_access";
const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";

#[no_mangle]
pub extern "C" fn mint() {
    mint_token::mint();
}

#[no_mangle]
pub extern "C" fn create() {
    mint_token::create();
}

#[no_mangle]
pub extern "C" fn balance() {
    mint_token::balance();
}

#[no_mangle]
pub extern "C" fn transfer() {
    mint_token::transfer();
}

#[no_mangle]
pub extern "C" fn bond() {
    mint_token::bond();
}

#[no_mangle]
pub extern "C" fn unbond() {
    mint_token::unbond();
}

#[no_mangle]
pub extern "C" fn unbond_timer_advance() {
    mint_token::unbond_timer_advance();
}

#[no_mangle]
pub extern "C" fn slash() {
    mint_token::slash();
}

#[no_mangle]
pub extern "C" fn release_founder_stake() {
    mint_token::release_founder_stake();
}

#[no_mangle]
pub extern "C" fn install() {
    let entry_points = mint_token::get_entry_points();

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let founder_purses = {
        let genesis_validators: BTreeMap<AccountHash, U512> =
            runtime::get_named_arg(ARG_GENESIS_VALIDATORS);

        let mut founder_purses = FounderPurses::new();
        for (account_hash, amount) in genesis_validators {
            let purse = MintContract.mint(amount).unwrap_or_revert();
            founder_purses.insert(account_hash, (purse, false));
        }

        founder_purses
    };

    let mut named_keys = NamedKeys::new();
    named_keys.insert(
        BID_PURSES_KEY.into(),
        storage::new_uref(BidPurses::new()).into(),
    );
    named_keys.insert(
        FOUNDER_PURSES_KEY.into(),
        storage::new_uref(founder_purses).into(),
    );
    named_keys.insert(
        UNBONDING_PURSES_KEY.into(),
        storage::new_uref(UnbondingPurses::new()).into(),
    );

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t((contract_package_hash, contract_key)).unwrap_or_revert();
    runtime::ret(return_value);
}
