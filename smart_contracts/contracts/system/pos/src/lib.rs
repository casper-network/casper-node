#![cfg_attr(not(test), no_std)]

extern crate alloc;

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    proof_of_stake::{MintProvider, ProofOfStake, RuntimeProvider},
    BlockTime, CLValue, Key, Phase, TransferResult, URef, U512,
};

pub const METHOD_GET_PAYMENT_PURSE: &str = "get_payment_purse";
pub const METHOD_SET_REFUND_PURSE: &str = "set_refund_purse";
pub const METHOD_GET_REFUND_PURSE: &str = "get_refund_purse";
pub const METHOD_FINALIZE_PAYMENT: &str = "finalize_payment";

pub const ARG_AMOUNT: &str = "amount";
pub const ARG_PURSE: &str = "purse";
pub const ARG_ACCOUNT_KEY: &str = "account";

pub struct ProofOfStakeContract;

impl MintProvider for ProofOfStakeContract {
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> TransferResult {
        system::transfer_from_purse_to_account(source, target, amount)
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()> {
        system::transfer_from_purse_to_purse(source, target, amount).map_err(|_| ())
    }

    fn balance(&mut self, purse: URef) -> Option<U512> {
        system::get_balance(purse)
    }
}

impl RuntimeProvider for ProofOfStakeContract {
    fn get_key(&self, name: &str) -> Option<Key> {
        runtime::get_key(name)
    }

    fn put_key(&mut self, name: &str, key: Key) {
        runtime::put_key(name, key)
    }

    fn remove_key(&mut self, name: &str) {
        runtime::remove_key(name)
    }

    fn get_phase(&self) -> Phase {
        runtime::get_phase()
    }

    fn get_block_time(&self) -> BlockTime {
        runtime::get_blocktime()
    }

    fn get_caller(&self) -> AccountHash {
        runtime::get_caller()
    }
}

impl ProofOfStake for ProofOfStakeContract {}

pub fn get_payment_purse() {
    let pos_contract = ProofOfStakeContract;
    let rights_controlled_purse = pos_contract.get_payment_purse().unwrap_or_revert();
    let return_value = CLValue::from_t(rights_controlled_purse).unwrap_or_revert();
    runtime::ret(return_value);
}

pub fn set_refund_purse() {
    let mut pos_contract = ProofOfStakeContract;

    let refund_purse: URef = runtime::get_named_arg(ARG_PURSE);
    pos_contract
        .set_refund_purse(refund_purse)
        .unwrap_or_revert();
}

pub fn get_refund_purse() {
    let pos_contract = ProofOfStakeContract;
    // We purposely choose to remove the access rights so that we do not
    // accidentally give rights for a purse to some contract that is not
    // supposed to have it.
    let maybe_refund_purse = pos_contract.get_refund_purse().unwrap_or_revert();
    let return_value = CLValue::from_t(maybe_refund_purse).unwrap_or_revert();
    runtime::ret(return_value);
}

pub fn finalize_payment() {
    let mut pos_contract = ProofOfStakeContract;

    let amount_spent: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let account: AccountHash = runtime::get_named_arg(ARG_ACCOUNT_KEY);
    pos_contract
        .finalize_payment(amount_spent, account)
        .unwrap_or_revert();
}
