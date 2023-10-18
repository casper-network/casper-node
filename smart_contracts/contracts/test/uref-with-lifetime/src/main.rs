#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::storage;
use casper_types::{EraId, Lifetime};

#[no_mangle]
pub extern "C" fn call() {
    let uref = storage::new_uref_with_lifetime(1, Lifetime::Finite(EraId::new(1)));
    let value = storage::read(uref);
    assert_eq!(value, Ok(Some(1)));

    let uref = storage::new_uref_with_lifetime(2, Lifetime::Indefinite);
    let value = storage::read(uref);
    assert_eq!(value, Ok(Some(2)));
}
