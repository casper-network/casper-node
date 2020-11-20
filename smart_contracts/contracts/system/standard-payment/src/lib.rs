#![no_std]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    proof_of_stake::METHOD_GET_PAYMENT_PURSE,
    standard_payment::{
        AccountProvider, MintProvider, ProofOfStakeProvider, StandardPayment, ARG_AMOUNT,
    },
    ApiError, RuntimeArgs, URef, U512,
};

struct StandardPaymentContract;

impl AccountProvider for StandardPaymentContract {
    fn get_main_purse(&self) -> Result<URef, ApiError> {
        Ok(account::get_main_purse())
    }
}

impl MintProvider for StandardPaymentContract {
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError> {
        system::transfer_from_purse_to_purse(source, target, amount, None)
    }
}

impl ProofOfStakeProvider for StandardPaymentContract {
    fn get_payment_purse(&mut self) -> Result<URef, ApiError> {
        let pos_pointer = system::get_proof_of_stake();
        let payment_purse = runtime::call_contract(
            pos_pointer,
            METHOD_GET_PAYMENT_PURSE,
            RuntimeArgs::default(),
        );
        Ok(payment_purse)
    }
}

impl StandardPayment for StandardPaymentContract {}

pub fn delegate() {
    let mut standard_payment_contract = StandardPaymentContract;

    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    standard_payment_contract.pay(amount).unwrap_or_revert();
}
