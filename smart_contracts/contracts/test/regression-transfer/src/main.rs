#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, runtime_args, system::mint, RuntimeArgs, URef, U512};

fn call_mint_transfer(
    to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
) -> Result<(), mint::Error> {
    let args = runtime_args! {
        mint::ARG_TO => to,
        mint::ARG_SOURCE => source,
        mint::ARG_TARGET => target,
        mint::ARG_AMOUNT => amount + U512::one(),
        mint::ARG_ID => id,
    };
    runtime::call_contract(system::get_mint(), mint::METHOD_TRANSFER, args)
}

#[no_mangle]
pub extern "C" fn call() {
    let to: Option<AccountHash> = runtime::get_named_arg(mint::ARG_TO);
    let source: URef = account::get_main_purse();
    let target: URef = runtime::get_named_arg(mint::ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(mint::ARG_AMOUNT);
    let id: Option<u64> = runtime::get_named_arg(mint::ARG_ID);

    call_mint_transfer(to, source, target, amount, id).unwrap_or_revert();
}
