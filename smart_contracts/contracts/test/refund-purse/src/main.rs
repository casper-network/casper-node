#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, system::handle_payment, ContractHash, RuntimeArgs, URef, U512};

pub const ARG_PURSE: &str = "purse";
const ARG_PAYMENT_AMOUNT: &str = "payment_amount";

fn set_refund_purse(contract_hash: ContractHash, p: &URef) {
    runtime::call_contract(
        contract_hash,
        handle_payment::METHOD_SET_REFUND_PURSE,
        runtime_args! {
            ARG_PURSE => *p,
        },
    )
}

fn get_payment_purse(handle_payment: ContractHash) -> URef {
    runtime::call_contract(
        handle_payment,
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        runtime_args! {},
    )
}

fn submit_payment(handle_payment: ContractHash, amount: U512) {
    let payment_purse = get_payment_purse(handle_payment);
    let main_purse = account::get_main_purse();
    system::transfer_from_purse_to_purse(main_purse, payment_purse, amount, None).unwrap_or_revert()
}

#[no_mangle]
pub extern "C" fn call() {
    let handle_payment = system::get_handle_payment();

    let refund_purse = system::create_purse();
    {
        // it should return Some(x) after calling set_refund_purse(x)
        set_refund_purse(handle_payment, &refund_purse);
    }
    {
        let refund_purse = system::create_purse();

        set_refund_purse(handle_payment, &refund_purse);

        let payment_amount: U512 = runtime::get_named_arg(ARG_PAYMENT_AMOUNT);
        submit_payment(handle_payment, payment_amount);
    }
}
