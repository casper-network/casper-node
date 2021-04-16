#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, RuntimeArgs, URef, U512};

#[repr(u16)]
enum Error {
    TransferFromSourceToPayment = 0,
    TransferFromPaymentToSource,
    GetBalance,
    CheckBalance,
}

const ARG_AMOUNT: &str = "amount";
const ENTRY_POINT_GET_PAYMENT_PURSE: &str = "get_payment_purse";

#[no_mangle]
pub extern "C" fn call() {
    // amount passed to payment contract
    let payment_fund: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let contract_hash = system::get_handle_payment();
    let source_purse = account::get_main_purse();
    let payment_amount: U512 = 100.into();
    let payment_purse: URef = runtime::call_contract(
        contract_hash,
        ENTRY_POINT_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    // can deposit
    system::transfer_from_purse_to_purse(source_purse, payment_purse, payment_amount, None)
        .unwrap_or_revert_with(ApiError::User(Error::TransferFromSourceToPayment as u16));

    let payment_balance = system::get_purse_balance(payment_purse)
        .unwrap_or_revert_with(ApiError::User(Error::GetBalance as u16));

    if payment_balance.saturating_sub(payment_fund) != payment_amount {
        runtime::revert(ApiError::User(Error::CheckBalance as u16))
    }

    // cannot withdraw
    if system::transfer_from_purse_to_purse(payment_purse, source_purse, payment_amount, None)
        .is_ok()
    {
        runtime::revert(ApiError::User(Error::TransferFromPaymentToSource as u16));
    }
}
