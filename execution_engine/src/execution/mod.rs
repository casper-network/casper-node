//! Code execution.
mod address_generator;
mod error;
#[macro_use]
mod executor;

pub(crate) use self::executor::{DirectSystemContractCall, Executor};
pub use self::{address_generator::AddressGenerator, error::Error};
