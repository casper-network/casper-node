#![no_std]

use casperlabs_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{account::AccountHash, ApiError, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

#[repr(u32)]
enum CustomError {
    AlreadyFunded = 1,
}

/// Executes token transfer to supplied account hash.
/// Revert status codes:
/// 1 - requested transfer to already funded account hash.
#[no_mangle]
pub fn delegate() {
    let account_hash: AccountHash = runtime::get_named_arg(ARG_TARGET);

    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    // Maybe we will decide to allow multiple funds up until some maximum value.
    let already_funded = storage::read_local::<AccountHash, U512>(&account_hash)
        .unwrap_or_default()
        .is_some();

    if already_funded {
        runtime::revert(ApiError::User(CustomError::AlreadyFunded as u16));
    } else {
        system::transfer_to_account(account_hash, amount).unwrap_or_revert();
        // Transfer successful; Store the fact of funding in the local state.
        storage::write_local(account_hash, amount);
    }
}
