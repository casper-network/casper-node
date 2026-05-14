//! EVM-native types shared by Casper components.
//!
//! This module intentionally exposes Casper-owned wrapper types instead of
//! executor or `revm` types. Ethereum transaction decoding and secp256k1 sender
//! recovery are performed here so downstream crates can validate signed RLP
//! without depending on the executor implementation.

mod account;
mod address;
mod config;
mod eth_u256;
mod evm_addr;
mod hash;
mod receipt;
mod topic;
mod transaction;

pub use account::{deterministic_purse, StorageAddr, EMPTY_CODE_HASH};
pub use address::{Address, ADDRESS_LENGTH};
pub use config::{EvmConfig, EvmSpec};
pub use eth_u256::EthU256;
pub use evm_addr::EvmAddr;
pub use hash::{Hash, HASH_LENGTH};
pub use receipt::{HaltReason, Log, OutOfGasError, Receipt, ReceiptStatus};
pub use topic::Topic;
pub use transaction::{
    Transaction, TransactionError, TransactionHash, TransactionKind, EIP1559_TRANSACTION_TYPE_ID,
    EIP2930_TRANSACTION_TYPE_ID, EIP4844_TRANSACTION_TYPE_ID, EIP7702_TRANSACTION_TYPE_ID,
    LEGACY_TRANSACTION_TYPE_ID,
};
