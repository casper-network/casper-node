#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{AddressableEntityHash, Key, RuntimeArgs, URef, U512};

const GET_PAYMENT_PURSE_NAME: &str = "get_payment_purse";
const HASH_KEY_NAME: &str = "contract_own_funds_hash";
const ARG_AMOUNT: &str = "amount";

fn get_payment_purse() -> URef {
    let contract_hash = get_entity_hash_name();
    runtime::call_contract(
        contract_hash,
        GET_PAYMENT_PURSE_NAME,
        RuntimeArgs::default(),
    )
}

fn get_entity_hash_name() -> AddressableEntityHash {
    runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_entity_hash_addr)
        .map(AddressableEntityHash::new)
        .unwrap_or_revert()
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let payment_purse = get_payment_purse();

    system::transfer_from_purse_to_purse(account::get_main_purse(), payment_purse, amount, None)
        .unwrap_or_revert();
}
