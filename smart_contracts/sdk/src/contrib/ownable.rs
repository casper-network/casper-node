//! This module provides an implementation of the Ownable pattern for smart contracts.
//!
//! The Ownable pattern is a common design pattern in smart contracts that allows for
//! a single owner to control the contract. This module provides a simple implementation
//! of this pattern, allowing for ownership to be transferred or renounced.
#[allow(unused_imports)]
use crate as casper_contract_sdk;
use crate::{casper::Entity, macros::casper};

/// The state of the Ownable contract, which contains the owner of the contract.
#[casper(path = crate)]
pub struct OwnableState {
    owner: Option<Entity>,
}

impl Default for OwnableState {
    fn default() -> Self {
        Self {
            owner: Some(crate::casper::get_caller()),
        }
    }
}

/// Represents the possible errors that can occur during ownership operations.
#[casper(path = crate)]
pub enum OwnableError {
    /// The caller is not authorized to perform the action.
    NotAuthorized,
}

/// The Ownable trait provides a simple ownership model for smart contracts.
/// It allows for a single owner to be set, and provides functions to transfer or renounce
/// ownership.
#[casper(path = crate, export = true)]
pub trait Ownable {
    #[casper(private)]
    fn state(&self) -> &OwnableState;
    #[casper(private)]
    fn state_mut(&mut self) -> &mut OwnableState;

    /// Checks if the caller is the owner of the contract.
    ///
    /// This function is used to restrict access to certain functions to only the owner.
    #[casper(private)]
    fn only_owner(&self) -> Result<(), OwnableError> {
        let caller = crate::casper::get_caller();
        match self.state().owner {
            Some(owner) if caller != owner => {
                return Err(OwnableError::NotAuthorized);
            }
            None => {
                return Err(OwnableError::NotAuthorized);
            }
            Some(_owner) => {}
        }
        Ok(())
    }

    /// Transfers ownership of the contract to a new owner.
    #[casper(revert_on_error)]
    fn transfer_ownership(&mut self, new_owner: Entity) -> Result<(), OwnableError> {
        self.only_owner()?;
        self.state_mut().owner = Some(new_owner);
        Ok(())
    }

    /// Returns the current owner of the contract.
    fn owner(&self) -> Option<Entity> {
        self.state().owner
    }

    /// Renounces ownership of the contract, making it no longer owned by any entity.
    ///
    /// This function can only be called by the current owner of the contract
    /// once the contract is deployed. After calling this function, the contract
    /// will no longer have an owner, and no entity will be able to call
    /// functions that require ownership.
    #[casper(revert_on_error)]
    fn renounce_ownership(&mut self) -> Result<(), OwnableError> {
        self.only_owner()?;
        self.state_mut().owner = None;
        Ok(())
    }
}
