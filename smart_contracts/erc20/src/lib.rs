//! Implementation of a ERC20 Token Standard.
#![warn(missing_docs)]
#![feature(once_cell)]
#![no_std]

extern crate alloc;

pub mod address;
mod allowances;
mod balances;
pub mod constants;
mod detail;
pub mod entry_points;
pub mod error;

use alloc::string::{String, ToString};

use once_cell::unsync::OnceCell;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{contracts::NamedKeys, EntryPoints, Key, URef, U256};

pub use address::Address;
use constants::{
    ALLOWANCES_KEY, BALANCES_KEY, DECIMALS_KEY, ERC20_TOKEN_CONTRACT_KEY, NAME_KEY, SYMBOL_KEY,
    TOTAL_SUPPLY_KEY,
};
use error::Error;

/// Implementation of a ERC20 token standard on the Casper platform.
#[derive(Default)]
pub struct ERC20 {
    balances_uref: OnceCell<URef>,
    allowances_uref: OnceCell<URef>,
}

impl ERC20 {
    fn new(balances_uref: URef, allowances_uref: URef) -> Self {
        Self {
            balances_uref: balances_uref.into(),
            allowances_uref: allowances_uref.into(),
        }
    }

    fn get_balances_uref(&self) -> &URef {
        self.balances_uref.get_or_init(balances::get_balances_uref)
    }

    fn read_balance(&self, owner: &Address) -> U256 {
        balances::read_balance_from(self.get_balances_uref(), owner)
    }

    fn write_balance(&mut self, owner: &Address, amount: U256) {
        balances::write_balance_to(self.get_balances_uref(), owner, amount)
    }

    fn get_allowances_uref(&self) -> &URef {
        self.allowances_uref
            .get_or_init(allowances::get_allowances_uref)
    }

    fn read_allowance(&self, owner: &Address, spender: &Address) -> U256 {
        allowances::read_allowance_from(self.get_allowances_uref(), owner, spender)
    }

    fn write_allowance(&mut self, owner: &Address, spender: &Address, amount: U256) {
        allowances::write_allowance_to(self.get_allowances_uref(), owner, spender, amount)
    }

    fn transfer_balance(
        &mut self,
        sender: &Address,
        recipient: &Address,
        amount: U256,
    ) -> Result<(), Error> {
        balances::transfer_balance(self.get_balances_uref(), sender, recipient, amount)
    }

    /// Returns name of the token.
    pub fn name(&self) -> String {
        detail::read_from(NAME_KEY)
    }

    /// Returns symbol of the token.
    pub fn symbol(&self) -> String {
        detail::read_from(SYMBOL_KEY)
    }

    /// Returns decimals of the token.
    pub fn decimals(&self) -> u8 {
        detail::read_from(DECIMALS_KEY)
    }

    /// Returns total supply of the token.
    pub fn total_supply(&self) -> U256 {
        detail::read_from(TOTAL_SUPPLY_KEY)
    }

    /// Checks balance of an owner.
    pub fn balance_of(&self, owner: &Address) -> U256 {
        self.read_balance(owner)
    }

    /// Transfer tokens from the caller to the `recipient`.
    pub fn transfer(&mut self, recipient: &Address, amount: U256) -> Result<(), Error> {
        let sender = detail::get_immediate_caller_address()?;

        self.transfer_balance(&sender, recipient, amount)?;

        Ok(())
    }

    /// Transfer tokens from `owner` address to the `recipient` address if required `amount` was
    /// approved before to be spend by the direct caller.
    ///
    /// This operation should decrement approved amount on the `owner`, and increase balance on the
    /// `recipient`.
    pub fn transfer_from(
        &mut self,
        owner: Address,
        recipient: Address,
        amount: U256,
    ) -> Result<(), Error> {
        let spender = detail::get_immediate_caller_address()?;

        let new_spender_allowance = {
            let spender_allowance = self.read_allowance(&owner, &spender);
            spender_allowance
                .checked_sub(amount)
                .ok_or(Error::InsufficientAllowance)?
        };

        self.transfer_balance(&owner, &recipient, amount)?;

        self.write_allowance(&owner, &spender, new_spender_allowance);

        Ok(())
    }

    /// Allow other address to transfer caller's tokens.
    pub fn approve(&mut self, spender: &Address, amount: U256) -> Result<(), Error> {
        let owner = detail::get_immediate_caller_address()?;

        allowances::write_allowance_to(self.get_allowances_uref(), &owner, spender, amount);

        Ok(())
    }

    /// Returns the amount allowed to spend.
    pub fn allowance(&self, owner: Address, spender: Address) -> U256 {
        allowances::read_allowance_from(self.get_allowances_uref(), &owner, &spender)
    }

    /// Internal function that mints an amount of the token and assigns it to an account.
    ///
    /// # Security
    ///
    /// This offers no security whatsoever, and for all practical purposes user of this function is
    /// advised to NOT expose this function through a public entry point.
    pub fn mint(&mut self, owner: &Address, amount: U256) -> Result<(), Error> {
        let new_balance = {
            let balance = self.read_balance(owner);
            balance.checked_add(amount).ok_or(Error::Overflow)?
        };
        self.write_balance(owner, new_balance);
        Ok(())
    }

    /// Internal function that burns an amount of the token of a given account.
    ///
    /// # Security
    ///
    /// This offers no security whatsoever, and for all practical purposes user of this function is
    /// advised to NOT expose this function through a public entry point.
    pub fn burn(&mut self, owner: &Address, amount: U256) -> Result<(), Error> {
        let new_balance = {
            let balance = self.read_balance(owner);
            balance
                .checked_sub(amount)
                .ok_or(Error::InsufficientBalance)?
        };
        self.write_balance(owner, new_balance);
        Ok(())
    }

    /// Install contract with custom set of entry points.
    ///
    /// # Warning
    ///
    /// Contract developers should use [`ERC20::install`] instead, as it will create default set of
    /// ERC20 entry points. Using `install_custom` with different set of entry points is unsafe and
    /// might lead to problems with integrators such as wallets, and exchanges.
    #[doc(hidden)]
    pub fn install_custom(
        name: String,
        symbol: String,
        decimals: u8,
        initial_supply: U256,
        contract_key_name: &str,
        entry_points: EntryPoints,
    ) -> Result<ERC20, Error> {
        let balances_uref = storage::new_dictionary(BALANCES_KEY).unwrap_or_revert();
        let allowances_uref = storage::new_dictionary(ALLOWANCES_KEY).unwrap_or_revert();

        let named_keys = {
            let mut named_keys = NamedKeys::new();

            let name_key = {
                let name_uref = storage::new_uref(name).into_read();
                Key::from(name_uref)
            };

            let symbol_key = {
                let symbol_uref = storage::new_uref(symbol).into_read();
                Key::from(symbol_uref)
            };

            let decimals_key = {
                let decimals_uref = storage::new_uref(decimals).into_read();
                Key::from(decimals_uref)
            };

            let total_supply_key = {
                let total_supply_uref = storage::new_uref(initial_supply).into_read();
                Key::from(total_supply_uref)
            };

            let balances_dictionary_key = {
                // Sets up initial balance for the caller - either an account, or a contract.
                let caller = detail::get_caller_address()?;
                balances::write_balance_to(&balances_uref, &caller, initial_supply);

                runtime::remove_key(BALANCES_KEY);

                Key::from(balances_uref)
            };

            let allowances_dictionary_key = {
                runtime::remove_key(ALLOWANCES_KEY);

                Key::from(allowances_uref)
            };

            named_keys.insert(NAME_KEY.to_string(), name_key);
            named_keys.insert(SYMBOL_KEY.to_string(), symbol_key);
            named_keys.insert(DECIMALS_KEY.to_string(), decimals_key);
            named_keys.insert(BALANCES_KEY.to_string(), balances_dictionary_key);
            named_keys.insert(ALLOWANCES_KEY.to_string(), allowances_dictionary_key);
            named_keys.insert(TOTAL_SUPPLY_KEY.to_string(), total_supply_key);

            named_keys
        };

        let (contract_hash, _version) =
            storage::new_locked_contract(entry_points, Some(named_keys), None, None);

        // Hash of the installed contract will be reachable through named keys.
        runtime::put_key(contract_key_name, Key::from(contract_hash));

        Ok(ERC20::new(balances_uref, allowances_uref))
    }

    /// This is the main entry point of the contract.
    ///
    /// It should be called from within `fn call` of your contract. If no additional functionality
    /// is planned then user should specify None as `entry_points`. Any custom set of entrypoit
    ///
    /// # Arguments
    ///
    /// name, symbol, decimals -- properties of the contract
    /// initial_supply -- how many tokens owner of this contract should have
    /// entry_points -- entrypoints to use
    pub fn install(
        name: String,
        symbol: String,
        decimals: u8,
        initial_supply: U256,
    ) -> Result<ERC20, Error> {
        let default_entry_points = entry_points::get_default_entry_points();
        ERC20::install_custom(
            name,
            symbol,
            decimals,
            initial_supply,
            ERC20_TOKEN_CONTRACT_KEY,
            default_entry_points,
        )
    }
}
