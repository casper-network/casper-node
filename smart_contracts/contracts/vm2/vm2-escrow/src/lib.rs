#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{collections::Map, prelude::*};

#[casper]
pub enum Error {
    AccountsAllowedOnly,
    InsufficientBalance,
    Transfer,
}

/// This contract implements a simple escrow contract.
#[derive(PanicOnDefault)]
#[casper(contract_state)]
pub struct Escrow {
    balances: Map<Entity, u64>,
}

#[casper]
impl Escrow {
    #[casper(constructor)]
    pub fn new() -> Self {
        if casper::transferred_value() != 0 {
            revert!()
        }
        Self {
            balances: Map::new("balances"),
        }
    }
    #[casper(revert_on_error, payable)]
    pub fn deposit_tokens(&mut self) -> Result<(), Error> {
        let entity = casper::get_caller();
        if !entity.is_account() {
            return Err(Error::AccountsAllowedOnly);
        }

        let current_balance = self.balances.get(&entity).unwrap_or_default();
        let new_balance = current_balance + casper::transferred_value();
        log!(
            "balance of {:?} changed from {} to {}",
            entity,
            current_balance,
            new_balance
        );
        self.balances.insert(&entity, &new_balance);
        Ok(())
    }

    pub fn balance_of(&mut self, entity: Entity) -> u64 {
        self.balances.get(&entity).unwrap_or_default()
    }

    #[casper(revert_on_error)]
    pub fn withdraw_tokens(&mut self, amount: u64) -> Result<(), Error> {
        let entity = casper::get_caller();
        let current_balance = self.balances.get(&entity).unwrap_or_default();
        if current_balance < amount {
            return Err(Error::InsufficientBalance);
        }
        let new_balance = current_balance - amount;
        if new_balance == 0 {
            self.balances.remove(&entity);
        } else {
            self.balances.insert(&entity, &new_balance);
        }
        debug_assert!(entity.is_account(), "Entity must be an account");
        casper::transfer(&entity.entity_addr(), amount).unwrap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escrow() {
        let _escrow = Escrow::default();
    }
}
