#![no_std]

use casperlabs_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{account::AccountHash, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

/// Executes mote transfer to supplied account hash.
/// Transfers the requested amount.
pub fn delegate() {
    let account_hash: AccountHash = runtime::get_named_arg(ARG_TARGET);
    let transfer_amount: u64 = runtime::get_named_arg(ARG_AMOUNT);
    let u512_motes = U512::from(transfer_amount);
    system::transfer_to_account(account_hash, u512_motes).unwrap_or_revert();
}
