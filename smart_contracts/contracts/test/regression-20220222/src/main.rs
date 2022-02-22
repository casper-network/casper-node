#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, ApiError, URef, U512, URefAddr, AccessRights};

const ALICE_ADDR: AccountHash = AccountHash::new([42; 32]);

#[repr(u16)]
enum Error {
    Revert1 = 0,
    Revert2 = 1,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let alice_purse_addr: URefAddr = runtime::get_named_arg("alice_purse_addr");

    let alice_purse = URef::new(alice_purse_addr, AccessRights::ADD);

    if runtime::is_valid_uref(alice_purse) {
        // Shouldn't be valid uref
        runtime::revert(Error::Revert1);
    }

    let source = account::get_main_purse();

    let _failsafe = system::transfer_from_purse_to_account(source, ALICE_ADDR, U512::one(), None)
        .unwrap_or_revert();

    // if runtime::is_valid_uref(alice_purse) {
    //     // Shouldn't be escalated
    //     runtime::revert(Error::Revert2);
    // }

    // Should work
    runtime::put_key("put_key_with_add_should_work", alice_purse.into());

    let _key = runtime::get_key("put_key_with_add_should_work").unwrap_or_revert();

    // Should fail
    runtime::put_key("put_key_with_write_should_fail", alice_purse.into_write().into());
}
