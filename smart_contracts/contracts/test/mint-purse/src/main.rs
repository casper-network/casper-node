#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, system::mint, ApiError, RuntimeArgs, URef, U512};

const METHOD_MINT: &str = "mint";
const METHOD_BALANCE: &str = "balance";

const ARG_AMOUNT: &str = "amount";
const ARG_PURSE: &str = "purse";

#[repr(u16)]
enum Error {
    BalanceNotFound = 0,
    BalanceMismatch,
}

fn mint_purse(amount: U512) -> Result<URef, mint::Error> {
    runtime::call_contract(
        system::get_mint(),
        METHOD_MINT,
        runtime_args! {
            ARG_AMOUNT => amount,
        },
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = 12345.into();
    let new_purse = mint_purse(amount).unwrap_or_revert();

    let mint = system::get_mint();

    let balance: Option<U512> = runtime::call_contract(
        mint,
        METHOD_BALANCE,
        runtime_args! {
            ARG_PURSE => new_purse,
        },
    );

    match balance {
        None => runtime::revert(ApiError::User(Error::BalanceNotFound as u16)),
        Some(balance) if balance == amount => (),
        _ => runtime::revert(ApiError::User(Error::BalanceMismatch as u16)),
    }
}
