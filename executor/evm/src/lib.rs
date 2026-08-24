//! Casper EVM executor.
//!
//! This crate provides a small execution API over `TrackingCopy` and keeps
//! `revm` details behind internal adapter modules.

mod account_state;
mod db;
mod error;
mod executor;
mod outcome;
mod precompiles;
mod request;
mod state;
mod tx;

pub use error::{DbError, Error, Result};
pub use executor::EvmExecutor;
pub use outcome::{ExecutionOutcome, ExecutionStatus};
pub use request::{
    BlockContext, CallRequest, CallValidation, ExecuteKind, ExecuteRequest, SystemCallRequest,
};

use casper_types::evm;

pub use casper_types::evm::Log;

/// Keccak-256 hash of empty EVM bytecode.
pub const EMPTY_CODE_HASH: evm::Hash = evm::EMPTY_CODE_HASH;

/// Number of recent block hashes available to EVM `BLOCKHASH`.
pub const BLOCK_HASH_HISTORY: u64 = revm::primitives::BLOCK_HASH_HISTORY;
