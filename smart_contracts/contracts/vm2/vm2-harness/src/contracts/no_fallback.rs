use alloc::vec::Vec;
use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    abi::CasperABI,
    host::{self, Entity},
    log, revert,
    types::{Address, CallError},
    Contract, ContractHandle,
};

use crate::traits::{Fallback, FallbackExt, FallbackRef};

use super::harness::HarnessRef;

/// A contract that can't receive tokens through a plain `fallback` method.
#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug, Default)]
pub struct NoFallback {
    initial_balance: u64,
    received_balance: u64,
}

#[casper(contract)]
impl NoFallback {
    #[casper(constructor)]
    pub fn initialize() -> Self {
        Self {
            initial_balance: host::get_value(),
            received_balance: 0,
        }
    }

    pub fn hello(&self) -> &str {
        "Hello, World!"
    }

    #[casper(payable)]
    pub fn receive_funds(&mut self) {
        let value = host::get_value();
        self.received_balance += value;
    }
}
