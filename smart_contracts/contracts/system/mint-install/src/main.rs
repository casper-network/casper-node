#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys,
    mint::{self, ACCESS_KEY, BASE_ROUND_REWARD_KEY, HASH_KEY},
    CLValue, URef,
};

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
pub extern "C" fn read_base_round_reward() {
    mint_token::read_base_round_reward();
}

#[no_mangle]
pub extern "C" fn install() {
    let entry_points = mint_token::get_entry_points();

    let base_round_reward_uref: URef = {
        let base_round_reward = mint::initial_supply_motes() * mint::round_seigniorage_rate();
        storage::new_uref(base_round_reward.to_integer()) // TODO: pass as Ratio<U512>
    };

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY, access_uref.into());

    let named_keys = {
        let mut tmp = NamedKeys::new();
        tmp.insert(BASE_ROUND_REWARD_KEY.into(), base_round_reward_uref.into());
        tmp
    };

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t((contract_package_hash, contract_key)).unwrap_or_revert();
    runtime::ret(return_value);
}
