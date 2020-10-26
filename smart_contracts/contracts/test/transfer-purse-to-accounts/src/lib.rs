#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, URef, U512};

const ARG_SOURCE: &str = "source";
const ARG_TARGETS: &str = "targets";

pub fn delegate() {
    let source: URef = runtime::get_named_arg(ARG_SOURCE);
    let targets: BTreeMap<AccountHash, U512> = runtime::get_named_arg(ARG_TARGETS);

    for (target, amount) in targets {
        system::transfer_from_purse_to_account(source, target, amount).unwrap_or_revert();
    }
}
