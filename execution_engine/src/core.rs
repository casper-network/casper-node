#![allow(missing_docs)]

pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

pub const ADDRESS_LENGTH: usize = 32;

pub type Address = [u8; ADDRESS_LENGTH];
