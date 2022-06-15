#![no_std]
#![no_main]

// casper_contract is required for it's [global_alloc] as well as handlers (such as panic_handler)
use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, runtime_args, system::mint, RuntimeArgs, URef, U512};

fn mint_transfer(
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
) -> Result<(), mint::Error> {
    let args = runtime_args! {
        mint::ARG_TO => maybe_to,
        mint::ARG_SOURCE => source,
        mint::ARG_TARGET => target,
        mint::ARG_AMOUNT => amount,
        mint::ARG_ID => id,
    };
    let mint_hash = system::get_mint();
    runtime::call_contract(mint_hash, mint::METHOD_TRANSFER, args)
}

fn delegate() {
    let to: Option<AccountHash> = runtime::get_named_arg("to");
    let amount: U512 = runtime::get_named_arg("amount");
    let main_purse = account::get_main_purse();
    let target_purse = main_purse;
    let id: Option<u64> = None;
    mint_transfer(to, main_purse, target_purse, amount, id).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    delegate();
}
