//! EVM-native types shared by Casper components.
//!
//! This module intentionally exposes Casper-owned wrapper types instead of
//! executor or `revm` types. Ethereum transaction decoding and secp256k1 sender
//! recovery are performed here so downstream crates can validate signed RLP
//! without depending on the executor implementation.

mod account;
mod address;
mod config;
mod eip2935;
mod eip4788;
mod evm_addr;
mod hash;
mod receipt;
mod topic;
mod transaction;

pub use account::{deterministic_purse, StorageAddr, EMPTY_CODE_HASH};
pub use address::{Address, ADDRESS_LENGTH};
pub use eip2935::{
    block_hash_history_code_hash, BLOCK_HASH_HISTORY_ADDRESS, BLOCK_HASH_HISTORY_CODE,
};
pub use eip4788::{beacon_roots_code_hash, BEACON_ROOTS_ADDRESS, BEACON_ROOTS_CODE};
pub use hash::{Hash, HASH_LENGTH};
pub use receipt::{HaltReason, Log, OutOfGasError, Receipt, ReceiptStatus};
pub use topic::Topic;
pub use transaction::{
    SetCodeAuthorization, EIP1559_TRANSACTION_TYPE_ID, EIP2930_TRANSACTION_TYPE_ID,
    EIP4844_TRANSACTION_TYPE_ID, EIP7702_TRANSACTION_TYPE_ID, LEGACY_TRANSACTION_TYPE_ID,
};

// Evm-prefixed wrappers should be reached through the crate root
// (`casper_types::EvmFoo`), not through `casper_types::evm::EvmFoo`.
// They are re-exported here so the rest of `casper-types` can import them
// without going through the crate root.
pub use config::{EvmConfig, EvmSpec, DEFAULT_WEI_PER_MOTE, MINIMUM_WEI_PER_MOTE};
pub use evm_addr::EvmAddr;
pub use transaction::{
    EvmApproval, EvmTransaction, EvmTransactionError, EvmTransactionHash, EvmTransactionKind,
};
