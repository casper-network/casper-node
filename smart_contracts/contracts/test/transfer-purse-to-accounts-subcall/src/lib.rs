#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, runtime_args, Key, RuntimeArgs, URef, U512};

const ENTRYPOINT: &str = "transfer";
const ARG_SOURCE: &str = "source";
const ARG_TARGETS: &str = "targets";

const HASH_KEY_NAME: &str = "transfer_purse_to_accounts_hash";

pub fn delegate() {
    let source: URef = account::get_main_purse();
    let targets: BTreeMap<AccountHash, (U512, Option<u64>)> = runtime::get_named_arg(ARG_TARGETS);

    for (target, (amount, id)) in &targets {
        system::transfer_from_purse_to_account(source, *target, *amount, *id).unwrap_or_revert();
    }

    let contract_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .unwrap_or_revert();

    runtime::call_contract(
        contract_hash.into(),
        ENTRYPOINT,
        runtime_args! {
            ARG_SOURCE => source,
            ARG_TARGETS => targets
        },
    )
}
