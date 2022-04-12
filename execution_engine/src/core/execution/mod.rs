//! Code execution.
mod address_generator;
mod error;
#[macro_use]
mod executor;

pub use self::error::Error;
pub(crate) use self::{
    address_generator::AddressGenerator,
    executor::{DirectSystemContractCall, Executor},
};
