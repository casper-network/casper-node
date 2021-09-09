//! Implementation of allowances.
use alloc::{string::String, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{bytesrepr::ToBytes, URef, U256};

use crate::{constants::ALLOWANCES_KEY, detail, Address};

#[inline]
pub fn get_allowances_uref() -> URef {
    detail::get_uref(ALLOWANCES_KEY)
}

/// Creates a dictionary item key for a (owner, spender) pair.
fn make_dictionary_item_key(owner: &Address, spender: &Address) -> String {
    let mut preimage = Vec::new();
    preimage.append(&mut owner.to_bytes().unwrap_or_revert());
    preimage.append(&mut spender.to_bytes().unwrap_or_revert());

    let key_bytes = runtime::blake2b(&preimage);
    hex::encode(&key_bytes)
}

/// Writes an allowance for owner and spender for a specific amount.
pub fn write_allowance_to(
    allowances_uref: &URef,
    owner: &Address,
    spender: &Address,
    amount: U256,
) {
    let dictionary_item_key = make_dictionary_item_key(owner, spender);
    storage::dictionary_put(*allowances_uref, &dictionary_item_key, amount)
}

/// Reads an allowance for a owner and spender
pub fn read_allowance_from(allowances_uref: &URef, owner: &Address, spender: &Address) -> U256 {
    let dictionary_item_key = make_dictionary_item_key(owner, spender);
    storage::dictionary_get(*allowances_uref, &dictionary_item_key)
        .unwrap_or_revert()
        .unwrap_or_default()
}
