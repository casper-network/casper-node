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

#[repr(u32)]
#[derive(Debug, BorshSerialize, BorshDeserialize, PartialEq, CasperABI, Clone)]
#[borsh(use_discriminant = true)]
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

#[derive(Debug, Default, BorshSerialize, BorshDeserialize, PartialEq, CasperABI, Clone)]
pub enum FallbackHandler {
    /// Accept tokens and do nothing.
    #[default]
    AcceptTokens,
    /// Reject tokens with revert.
    RejectWithRevert,
    /// Reject tokens with trap.
    RejectWithTrap,
}

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug, Default)]
#[casper(impl_traits(Fallback))]
pub struct TokenOwnerContract {
    initial_balance: u64,
    received_tokens: u64,
    fallback_handler: FallbackHandler,
}

#[casper(contract)]
impl TokenOwnerContract {
    #[casper(constructor)]
    pub fn initialize() -> Self {
        Self {
            initial_balance: host::get_value(),
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
        let self_balance = host::get_balance_of(&Entity::Contract(self_address));
        let res = ContractHandle::<HarnessRef>::from_address(contract_address)
            .build_call()
            .with_value(amount)
            .call(|harness| harness.deposit(self_balance))?;
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
        let self_balance = host::get_balance_of(&self_entity);

        let res = ContractHandle::<HarnessRef>::from_address(contract_address)
            .build_call()
            .call(|harness| {
                // Be careful about re-entrancy here: we are calling a contract that can call back while we're still not done with this entry point.
                // If &mut self is used, then the proc macro will save the state while the state was already saved at the end of `receive()` call.
                // To protect against re-entrancy attacks, please use `&self` or `self`.
                harness.withdraw(self_balance, amount)
            });

        let res = res?;

        match &res {
            Ok(()) => {
                log!("Token owner withdrew {amount} from {contract_address:?}");
                assert_eq!(
                    host::get_balance_of(&self_entity),
                    self_balance + amount,
                    "Balance should change"
                );
            }
            Err(e) => {
                log!("Token owner failed to withdraw {amount} from {contract_address:?}: {e:?}");
                assert_eq!(
                    host::get_balance_of(&self_entity),
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

impl Fallback for TokenOwnerContract {
    fn fallback(&mut self) {
        match self.fallback_handler {
            FallbackHandler::AcceptTokens => {
                let value = host::get_value();
                log!(
                    "TokenOwnerContract received fallback entrypoint with value={}",
                    value
                );
                self.received_tokens += value;
            }
            FallbackHandler::RejectWithRevert => {
                // This will cause a revert.
                revert!();
            }
            FallbackHandler::RejectWithTrap => {
                // This will cause a trap.
                unreachable!("its a trap");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_contract::contract_api::runtime;
    use casper_types::U512;

    #[test]
    fn test_token_owner_contract() {}
}
