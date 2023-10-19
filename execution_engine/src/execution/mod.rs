//! Code execution.
mod error;
#[macro_use]
mod executor;

pub use self::error::Error;
pub(crate) use self::executor::{DirectSystemContractCall, Executor};
