//! EVM-native types shared by Casper components.
//!
//! This module intentionally exposes Casper-owned wrapper types instead of
//! executor or `revm` types. Ethereum transaction decoding and secp256k1 sender
//! recovery are performed here so downstream crates can validate signed RLP
//! without depending on the executor implementation.

mod account;
mod address;
mod config;
mod hash;
mod receipt;
mod transaction;

pub use account::{deterministic_purse, Account, ByteCode, StorageAddr, StorageValue};
pub use address::{Address, ADDRESS_LENGTH};
pub use config::{EvmConfig, EvmSpec};
pub use hash::{Hash, HASH_LENGTH};
pub use receipt::{HaltReason, Log, OutOfGasError, Receipt, ReceiptStatus};
pub use transaction::{
    Transaction, TransactionError, TransactionHash, TransactionKind, EIP4844_TRANSACTION_TYPE_ID,
    EIP7702_TRANSACTION_TYPE_ID,
};
