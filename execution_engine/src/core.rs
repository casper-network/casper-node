#![allow(missing_docs)]

pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

pub use tracking_copy::{validate_balance_proof, validate_query_proof, ValidationError};

pub const ADDRESS_LENGTH: usize = 32;

pub type Address = [u8; ADDRESS_LENGTH];
