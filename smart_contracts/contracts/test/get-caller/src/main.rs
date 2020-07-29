#![no_std]
#![no_main]

use casperlabs_contract::contract_api::runtime;
use casperlabs_types::account::AccountHash;

const ARG_ACCOUNT: &str = "account";

#[no_mangle]
pub extern "C" fn call() {
    let known_account_hash: AccountHash = runtime::get_named_arg(ARG_ACCOUNT);
    let caller_account_hash: AccountHash = runtime::get_caller();
    assert_eq!(
        caller_account_hash, known_account_hash,
        "caller account hash was not known account hash"
    );
}
