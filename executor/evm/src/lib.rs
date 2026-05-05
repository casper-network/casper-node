//! Casper EVM executor.
//!
//! This crate provides a small execution API over `TrackingCopy` and keeps
//! `revm` details behind internal adapter modules.

mod db;
mod error;
mod executor;
mod outcome;
mod request;
mod state;
mod tx;

pub use error::{DbError, Error, Result};
pub use executor::EvmExecutor;
pub use outcome::{ExecutionOutcome, ExecutionStatus, Log};
pub use request::{BlockContext, CallRequest, ExecuteKind, ExecuteRequest};

use casper_types::evm;

/// Keccak-256 hash of empty EVM bytecode.
pub const EMPTY_CODE_HASH: evm::Hash = evm::Hash::new([
    0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7, 0x03, 0xc0,
    0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04, 0x5d, 0x85, 0xa4, 0x70,
]);
