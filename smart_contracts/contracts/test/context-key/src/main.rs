#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ContractHash, Digest, Key};

fn get_entity_hash() -> ContractHash {
    storage::read_from_key::<Key>(Key::from(runtime::get_caller()))
        .unwrap_or_revert()
        .unwrap_or_revert()
        .into_contract_hash()
        .unwrap_or_revert()
}

#[no_mangle]
pub extern "C" fn call() {
    let ctx = storage::new_context_key(b"foobar", 1);
    let contract_hash = get_entity_hash();
    assert_eq!(ctx.owner(), contract_hash);
    assert_ne!(ctx.key_hash(), Digest::default());

    let value = storage::read_from_key(Key::Context(ctx));
    assert_eq!(value, Ok(Some(1)));

    storage::write_to_key(Key::Context(ctx), Some(2));
    let value = storage::read_from_key(Key::Context(ctx));
    assert_eq!(value, Ok(Some(2)));
}
