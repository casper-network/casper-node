//! CEP-18 token standard.
//!
//! This module implements the CEP-18 token standard, which is a fungible token standard
//! for the Casper blockchain. It provides a set of functions and traits for creating, transferring,
//! and managing fungible tokens.
//!
//! The CEP-18 standard is designed to be simple and efficient, allowing developers to easily
//! create and manage fungible tokens on the Casper blockchain. It includes support for
//! minting, burning, and transferring tokens, as well as managing allowances and balances.
//!
//! The standard also includes support for events, allowing developers to emit events
//! when tokens are transferred, minted, or burned. This allows for easy tracking
//! and monitoring of token activity on the blockchain.
//!
//! It only requires implementation of `CEP18` trait for your contract to receive already
//! implemented entry points.
//!
//! # Example CEP18 token contract
//!
//! ```rust
//! use casper_contract_sdk::prelude::*;
//! use casper_contract_sdk::contrib::cep18::{CEP18, CEP18State, CEP18Ext, Mintable, Burnable};
//! # use casper_contract_sdk::collections::Map;
//! # use casper_contract_sdk::macros::casper;
//! # use casper_contract_sdk::types::U256;
//!
//! #[casper(contract_state)]
//! struct MyToken {
//!    state: CEP18State,
//! }
//!
//! impl Default for MyToken {
//!   fn default() -> Self {
//!     Self {
//!       state: CEP18State::new("MyToken", "MTK", 18, U256::from(10_000_000_000u64)),
//!     }
//!   }
//! }
//!
//! #[casper]
//! impl MyToken {
//!   #[casper(constructor)]
//!   pub fn new() -> Self {
//!     let my_token = Self::default();
//!     // Perform extra initialization if needed i.e. mint tokens, set genesis balance holders etc.
//!     my_token
//!   }
//! }
//!
//! #[casper(path = casper_contract_sdk::contrib::cep18)]
//! impl CEP18 for MyToken {
//!   fn state(&self) -> &CEP18State {
//!     &self.state
//!   }
//!
//!   fn state_mut(&mut self) -> &mut CEP18State {
//!     &mut self.state
//!   }
//! }
//! ```
use bnum::types::U256;

use super::access_control::{AccessControl, AccessControlError, Role};
#[allow(unused_imports)]
use crate as casper_contract_sdk;
use crate::{collections::Map, macros::blake2b256, prelude::*};

/// While the code consuming this contract needs to define further error variants, it can
/// return those via the [`Error::User`] variant or equivalently via the [`ApiError::User`]
/// variant.
#[derive(Debug, PartialEq, Eq)]
#[casper]
pub enum Cep18Error {
    /// CEP-18 contract called from within an invalid context.
    InvalidContext,
    /// Spender does not have enough balance.
    InsufficientBalance,
    /// Spender does not have enough allowance approved.
    InsufficientAllowance,
    /// Operation would cause an integer overflow.
    Overflow,
    /// A required package hash was not specified.
    PackageHashMissing,
    /// The package hash specified does not represent a package.
    PackageHashNotPackage,
    /// An invalid event mode was specified.
    InvalidEventsMode,
    /// The event mode required was not specified.
    MissingEventsMode,
    /// An unknown error occurred.
    Phantom,
    /// Failed to read the runtime arguments provided.
    FailedToGetArgBytes,
    /// The caller does not have sufficient security access.
    InsufficientRights,
    /// The list of Admin accounts provided is invalid.
    InvalidAdminList,
    /// The list of accounts that can mint tokens is invalid.
    InvalidMinterList,
    /// The list of accounts with no access rights is invalid.
    InvalidNoneList,
    /// The flag to enable the mint and burn mode is invalid.
    InvalidEnableMBFlag,
    /// This contract instance cannot be initialized again.
    AlreadyInitialized,
    ///  The mint and burn mode is disabled.
    MintBurnDisabled,
    CannotTargetSelfUser,
    InvalidBurnTarget,
}

impl From<AccessControlError> for Cep18Error {
    fn from(error: AccessControlError) -> Self {
        match error {
            AccessControlError::NotAuthorized => Cep18Error::InsufficientRights,
        }
    }
}

#[casper(message, path = crate)]
pub struct Transfer {
    pub from: Option<Entity>,
    pub to: Entity,
    pub amount: U256,
}

#[casper(message, path = crate)]
pub struct Approve {
    pub owner: Entity,
    pub spender: Entity,
    pub amount: U256,
}

pub const ADMIN_ROLE: Role = blake2b256!("admin");
pub const MINTER_ROLE: Role = blake2b256!("minter");

#[casper(path = crate)]
pub struct CEP18State {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: U256,
    pub balances: Map<Entity, U256>,
    pub allowances: Map<(Entity, Entity), U256>,
    pub enable_mint_burn: bool,
}

impl CEP18State {
    fn transfer_balance(
        &mut self,
        sender: &Entity,
        recipient: &Entity,
        amount: U256,
    ) -> Result<(), Cep18Error> {
        if amount.is_zero() {
            return Ok(());
        }

        let sender_balance = self.balances.get(sender).unwrap_or_default();

        let new_sender_balance = sender_balance
            .checked_sub(amount)
            .ok_or(Cep18Error::InsufficientBalance)?;

        let recipient_balance = self.balances.get(recipient).unwrap_or_default();

        let new_recipient_balance = recipient_balance
            .checked_add(amount)
            .ok_or(Cep18Error::Overflow)?;

        self.balances.insert(sender, &new_sender_balance);
        self.balances.insert(recipient, &new_recipient_balance);
        Ok(())
    }
}

impl CEP18State {
    pub fn new(name: &str, symbol: &str, decimals: u8, total_supply: U256) -> CEP18State {
        CEP18State {
            name: name.to_string(),
            symbol: symbol.to_string(),
            decimals,
            total_supply,
            balances: Map::new("balances"),
            allowances: Map::new("allowances"),
            enable_mint_burn: false,
        }
    }
}

#[casper(path = crate, export = true)]
pub trait CEP18 {
    #[casper(private)]
    fn state(&self) -> &CEP18State;

    #[casper(private)]
    fn state_mut(&mut self) -> &mut CEP18State;

    fn name(&self) -> &str {
        &self.state().name
    }

    fn symbol(&self) -> &str {
        &self.state().symbol
    }

    fn decimals(&self) -> u8 {
        self.state().decimals
    }

    fn total_supply(&self) -> U256 {
        self.state().total_supply
    }

    fn balance_of(&self, address: Entity) -> U256 {
        self.state().balances.get(&address).unwrap_or_default()
    }

    fn allowance(&self, spender: Entity, owner: Entity) {
        self.state()
            .allowances
            .get(&(spender, owner))
            .unwrap_or_default();
    }

    #[casper(revert_on_error)]
    fn approve(&mut self, spender: Entity, amount: U256) -> Result<(), Cep18Error> {
        let owner = casper::get_caller();
        if owner == spender {
            return Err(Cep18Error::CannotTargetSelfUser);
        }
        let lookup_key = (owner, spender);
        self.state_mut().allowances.insert(&lookup_key, &amount);
        casper::emit(Approve {
            owner,
            spender,
            amount,
        })
        .expect("failed to emit message");
        Ok(())
    }

    #[casper(revert_on_error)]
    fn decrease_allowance(&mut self, spender: Entity, amount: U256) -> Result<(), Cep18Error> {
        let owner = casper::get_caller();
        if owner == spender {
            return Err(Cep18Error::CannotTargetSelfUser);
        }
        let lookup_key = (owner, spender);
        let allowance = self.state().allowances.get(&lookup_key).unwrap_or_default();
        let allowance = allowance.saturating_sub(amount);
        self.state_mut().allowances.insert(&lookup_key, &allowance);
        Ok(())
    }

    #[casper(revert_on_error)]
    fn increase_allowance(&mut self, spender: Entity, amount: U256) -> Result<(), Cep18Error> {
        let owner = casper::get_caller();
        if owner == spender {
            return Err(Cep18Error::CannotTargetSelfUser);
        }
        let lookup_key = (owner, spender);
        let allowance = self.state().allowances.get(&lookup_key).unwrap_or_default();
        let allowance = allowance.saturating_add(amount);
        self.state_mut().allowances.insert(&lookup_key, &allowance);
        Ok(())
    }

    #[casper(revert_on_error)]
    fn transfer(&mut self, recipient: Entity, amount: U256) -> Result<(), Cep18Error> {
        let sender = casper::get_caller();
        if sender == recipient {
            return Err(Cep18Error::CannotTargetSelfUser);
        }
        self.state_mut()
            .transfer_balance(&sender, &recipient, amount)?;

        // NOTE: This is operation is fallible, although it's not expected to fail under any
        // circumstances (number of topics per contract, payload size, topic size, number of
        // messages etc. are all under control).
        casper::emit(Transfer {
            from: Some(sender),
            to: recipient,
            amount,
        })
        .expect("failed to emit message");

        Ok(())
    }

    #[casper(revert_on_error)]
    fn transfer_from(
        &mut self,
        owner: Entity,
        recipient: Entity,
        amount: U256,
    ) -> Result<(), Cep18Error> {
        let spender = casper::get_caller();
        if owner == recipient {
            return Err(Cep18Error::CannotTargetSelfUser);
        }

        if amount.is_zero() {
            return Ok(());
        }

        let spender_allowance = self
            .state()
            .allowances
            .get(&(owner, spender))
            .unwrap_or_default();
        let new_spender_allowance = spender_allowance
            .checked_sub(amount)
            .ok_or(Cep18Error::InsufficientAllowance)?;

        self.state_mut()
            .transfer_balance(&owner, &recipient, amount)?;

        self.state_mut()
            .allowances
            .insert(&(owner, spender), &new_spender_allowance);

        casper::emit(Transfer {
            from: Some(owner),
            to: recipient,
            amount,
        })
        .expect("failed to emit message");

        Ok(())
    }
}

#[casper(path = crate, export = true)]
pub trait Mintable: CEP18 + AccessControl {
    #[casper(revert_on_error)]
    fn mint(&mut self, owner: Entity, amount: U256) -> Result<(), Cep18Error> {
        if !CEP18::state(self).enable_mint_burn {
            return Err(Cep18Error::MintBurnDisabled);
        }

        AccessControl::require_any_role(self, &[ADMIN_ROLE, MINTER_ROLE])?;

        let balance = CEP18::state(self).balances.get(&owner).unwrap_or_default();
        let new_balance = balance.checked_add(amount).ok_or(Cep18Error::Overflow)?;
        CEP18::state_mut(self).balances.insert(&owner, &new_balance);
        CEP18::state_mut(self).total_supply = CEP18::state(self)
            .total_supply
            .checked_add(amount)
            .ok_or(Cep18Error::Overflow)?;

        casper::emit(Transfer {
            from: None,
            to: owner,
            amount,
        })
        .expect("failed to emit message");

        Ok(())
    }
}

#[casper(path = crate, export = true)]
pub trait Burnable: CEP18 {
    #[casper(revert_on_error)]
    fn burn(&mut self, owner: Entity, amount: U256) -> Result<(), Cep18Error> {
        if !self.state().enable_mint_burn {
            return Err(Cep18Error::MintBurnDisabled);
        }

        if owner != casper::get_caller() {
            return Err(Cep18Error::InvalidBurnTarget);
        }

        let balance = self.state().balances.get(&owner).unwrap_or_default();
        let new_balance = balance.checked_add(amount).ok_or(Cep18Error::Overflow)?;
        self.state_mut().balances.insert(&owner, &new_balance);
        self.state_mut().total_supply = self
            .state()
            .total_supply
            .checked_sub(amount)
            .ok_or(Cep18Error::Overflow)?;
        Ok(())
    }
}
