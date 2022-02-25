//! The core of the smart contract execution logic.
pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

pub use tracking_copy::{validate_balance_proof, validate_query_proof, ValidationError};

/// The length of an address.
pub const ADDRESS_LENGTH: usize = 32;

/// Alias for an array of bytes that represents an address.
pub type Address = [u8; ADDRESS_LENGTH];

//TODO: Move both registries here.
