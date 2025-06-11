use casper_contract_sdk::prelude::*;

use casper_contract_macros::casper;
use casper_contract_sdk::{
    casper::{self, Entity},
    log, revert,
    types::{Address, CallError},
    ContractHandle,
};

use crate::traits::{Deposit, DepositExt};

use super::harness::HarnessRef;

#[derive(Debug, PartialEq)]
#[casper]
pub enum TokenOwnerError {
    CallError(CallError),
    DepositError(String),
    WithdrawError(String),
}

impl From<CallError> for TokenOwnerError {
    fn from(v: CallError) -> Self {
        Self::CallError(v)
    }
}

pub type Data = Vec<u8>; // TODO: CasperABI does not support generic parameters and it fails to compile, we need to support
                         // this in the macro

#[casper]
#[derive(Debug, Default, PartialEq)]
pub enum FallbackHandler {
    /// Accept tokens and do nothing.
    #[default]
    AcceptTokens,
    /// Reject tokens with revert.
    RejectWithRevert,
    /// Reject tokens with trap.
    RejectWithTrap,
    /// Reject tokens with a revert with data.
    RejectWithData(Data),
}

#[derive(Default)]
#[casper(contract_state)]
pub struct TokenOwnerContract {
    initial_balance: u64,
    received_tokens: u64,
    fallback_handler: FallbackHandler,
}

#[casper]
impl TokenOwnerContract {
    #[casper(constructor, payable)]
    pub fn token_owner_initialize() -> Self {
        Self {
            initial_balance: casper::transferred_value(),
            received_tokens: 0,
            fallback_handler: FallbackHandler::AcceptTokens,
        }
    }

    pub fn do_deposit(
        &self,
        self_address: Address,
        contract_address: Address,
        amount: u64,
    ) -> Result<(), TokenOwnerError> {
        let self_balance = casper::get_balance_of(&Entity::Contract(self_address));
        let res = ContractHandle::<HarnessRef>::from_address(contract_address)
            .build_call()
            .with_transferred_value(amount)
            .call(|harness| harness.perform_token_deposit(self_balance))?;
        match &res {
            Ok(()) => log!("Token owner deposited {amount} to {contract_address:?}"),
            Err(e) => log!("Token owner failed to deposit {amount} to {contract_address:?}: {e:?}"),
        }
        res.map_err(|error| TokenOwnerError::DepositError(error.to_string()))?;
        Ok(())
    }

    pub fn do_withdraw(
        &self,
        self_address: Address,
        contract_address: Address,
        amount: u64,
    ) -> Result<(), TokenOwnerError> {
        let self_entity = Entity::Contract(self_address);
        let self_balance = casper::get_balance_of(&self_entity);

        let res = ContractHandle::<HarnessRef>::from_address(contract_address)
            .build_call()
            .call(|harness| {
                // Be careful about re-entrancy here: we are calling a contract that can call back
                // while we're still not done with this entry point. If &mut self is
                // used, then the proc macro will save the state while the state was already saved
                // at the end of `receive()` call. To protect against re-entrancy
                // attacks, please use `&self` or `self`.
                harness.withdraw(self_balance, amount)
            });

        let res = res?;

        match &res {
            Ok(()) => {
                log!("Token owner withdrew {amount} from {contract_address:?}");
                assert_eq!(
                    casper::get_balance_of(&self_entity),
                    self_balance + amount,
                    "Balance should change"
                );
            }
            Err(e) => {
                log!("Token owner failed to withdraw {amount} from {contract_address:?}: {e:?}");
                assert_eq!(
                    casper::get_balance_of(&self_entity),
                    self_balance,
                    "Balance should NOT change"
                );
            }
        }

        res.map_err(|error| TokenOwnerError::WithdrawError(error.to_string()))?;
        Ok(())
    }

    pub fn total_received_tokens(&self) -> u64 {
        self.received_tokens
    }

    pub fn set_fallback_handler(&mut self, handler: FallbackHandler) {
        self.fallback_handler = handler;
    }
}

#[casper(path = crate::traits)]
impl Deposit for TokenOwnerContract {
    fn deposit(&mut self) {
        log!(
            "Received deposit with value = {} current handler is {:?}",
            casper::transferred_value(),
            self.fallback_handler
        );
        match std::mem::replace(&mut self.fallback_handler, FallbackHandler::AcceptTokens) {
            FallbackHandler::AcceptTokens => {
                let value = casper::transferred_value();
                log!(
                    "TokenOwnerContract received fallback entrypoint with value={}",
                    value
                );
                self.received_tokens += value;
            }
            FallbackHandler::RejectWithRevert => {
                // This will cause a revert.
                log!("TokenOwnerContract rejected with revert");
                revert!();
            }
            FallbackHandler::RejectWithTrap => {
                // This will cause a trap.
                unreachable!("its a trap");
            }
            FallbackHandler::RejectWithData(data) => {
                // This will cause a revert with data.
                revert!(data);
            }
        }
    }
}
