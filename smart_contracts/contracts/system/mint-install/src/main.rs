#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use num_rational::Ratio;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys,
    mint::{ACCESS_KEY, ARG_ROUND_SEIGNIORAGE_RATE, HASH_KEY, ROUND_SEIGNIORAGE_RATE_KEY},
    CLValue, U512,
};

#[no_mangle]
pub extern "C" fn mint() {
    mint_token::mint();
}

#[no_mangle]
pub extern "C" fn reduce_total_supply() {
    mint_token::reduce_total_supply();
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
pub extern "C" fn read_base_round_reward() {
    mint_token::read_base_round_reward();
}

#[no_mangle]
pub extern "C" fn install() {
    let round_seigniorage_rate: Ratio<U512> = runtime::get_named_arg(ARG_ROUND_SEIGNIORAGE_RATE);

    let entry_points = mint_token::get_entry_points();

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY, access_uref.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let round_seigniorage_rate_uref = storage::new_uref(round_seigniorage_rate);
        named_keys.insert(
            ROUND_SEIGNIORAGE_RATE_KEY.to_string(),
            round_seigniorage_rate_uref.into(),
        );
        named_keys
    };

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t((contract_package_hash, contract_key)).unwrap_or_revert();
    runtime::ret(return_value);
}
