#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, system::mint, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_BURN_AMOUNT: &str = "burn_amount";

/// Calls mint `burn` directly on the caller's main purse for the inner `burn_amount`. The outer
/// `amount` runtime arg becomes the transaction's approved spending limit, so this contract is
/// the natural fixture for audit-confirmed-83: with `amount` small and `burn_amount` large, the
/// runtime must reject the burn with `mint::Error::UnapprovedSpendingAmount` instead of allowing
/// the larger amount through.
#[no_mangle]
pub extern "C" fn call() {
    let _outer_amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let burn_amount: U512 = runtime::get_named_arg(ARG_BURN_AMOUNT);
    let main_purse = account::get_main_purse();
    let mint_hash = system::get_mint();
    let args = runtime_args! {
        mint::ARG_PURSE => main_purse,
        mint::ARG_AMOUNT => burn_amount,
    };
    let result: Result<(), mint::Error> =
        runtime::call_contract(mint_hash, mint::METHOD_BURN, args);
    result.unwrap_or_revert();
}
