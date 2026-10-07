#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, system::mint, URef, U512};

const ARG_AMOUNT: &str = "amount";

fn burn(uref: URef, amount: U512) {
    let mint_hash = system::get_mint();
    let args = runtime_args! {
        mint::ARG_PURSE => uref,
        mint::ARG_AMOUNT => amount,
    };
    let result: Result<(), mint::Error> =
        runtime::call_contract(mint_hash, mint::METHOD_BURN, args);
    result.unwrap_or_revert();
}

/// Session that calls mint `burn` on the caller main purse and then deliberately runs out of gas
/// by repeatedly writing a growing value. Used as a regression contract for audit-confirmed-152:
/// after the fix, the failed (`Out of gas error`) execution must NOT commit the burn effect.
#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    burn(account::get_main_purse(), amount);

    // Now exhaust the gas budget. Each loop iteration creates a new uref + write.
    loop {
        let _uref = storage::new_uref(0u64);
    }
}
