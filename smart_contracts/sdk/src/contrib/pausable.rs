//! This module provides a trait for pausable contracts.
//!
//! The `Pausable` trait allows contracts to be paused and unpaused, which can be useful
//! in scenarios where the contract needs to be temporarily disabled for maintenance or
//! security reasons. The trait provides methods to check the current pause state, as well
//! as to pause and unpause the contract.
//!
//! The `Pausable` trait is designed to be used with the `casper` macro, which generates
//! the necessary boilerplate code for the contract.
//!
//! For security reasons you may want to combine [`AccessControl`] or [`Ownable`] with
//! this trait to ensure that only selected entities can manage the pause state.
use crate::{self as casper_contract_sdk, casper, casper::Entity, macros::casper};

#[casper]
pub struct PausedState {
    paused: bool,
}

#[casper(path = crate)]
pub enum PausableError {
    EnforcedPause,
    ExpectedPause,
}

/// The `Paused` event is emitted when the contract is paused.
#[casper(message, path = crate)]
pub struct Paused {
    entity: Entity,
}

/// The `Unpaused` event is emitted when the contract is unpaused.
#[casper(message, path = crate)]
pub struct Unpaused {
    entity: Entity,
}

/// Pausable is a trait that provides a simple way to pause and unpause a contract.
#[casper(path = crate, export = true)]
pub trait Pausable {
    /// The state of the contract, which contains the paused state.
    #[casper(private)]
    fn state(&self) -> &PausedState;
    /// The mutable state of the contract, which allows modifying the paused state.
    #[casper(private)]
    fn state_mut(&mut self) -> &mut PausedState;

    /// Checks if the contract is paused.
    #[casper(private)]
    fn paused(&self) -> bool {
        self.state().paused
    }

    #[casper(private)]
    fn pause(&mut self) -> Result<(), PausableError> {
        self.enforce_unpaused()?;
        self.state_mut().paused = true;
        casper::emit(Paused {
            entity: casper::get_caller(),
        })
        .expect("Emit");
        Ok(())
    }

    #[casper(private)]
    fn unpause(&mut self) -> Result<(), PausableError> {
        self.enforce_paused()?;
        self.state_mut().paused = false;
        casper::emit(Unpaused {
            entity: casper::get_caller(),
        })
        .expect("Emit");
        Ok(())
    }

    #[casper(private)]
    fn enforce_paused(&self) -> Result<(), PausableError> {
        if self.paused() {
            Ok(())
        } else {
            Err(PausableError::ExpectedPause)
        }
    }

    #[casper(private)]
    fn enforce_unpaused(&self) -> Result<(), PausableError> {
        if !self.paused() {
            Ok(())
        } else {
            Err(PausableError::EnforcedPause)
        }
    }
}
