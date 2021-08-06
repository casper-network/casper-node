//! Execution of smart contracts.
mod address_generator;
mod error;
#[macro_use]
mod executor;
#[cfg(test)]
mod tests;

pub use self::{
    address_generator::{AddressGenerator, AddressGeneratorBuilder},
    error::Error,
    executor::{DirectSystemContractCall, Executor},
};
