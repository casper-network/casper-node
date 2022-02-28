//! The core of the smart contract execution logic.
pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

use std::collections::BTreeMap;

use casper_hashing::Digest;
use casper_types::ContractHash;

pub use tracking_copy::{validate_balance_proof, validate_query_proof, ValidationError};

/// The length of an address.
pub const ADDRESS_LENGTH: usize = 32;

/// Alias for an array of bytes that represents an address.
pub type Address = [u8; ADDRESS_LENGTH];

/// Type alias for the system contract registry.
pub type SystemContractRegistry = BTreeMap<String, ContractHash>;

/// Type alias for the chainspec registry.
pub type ChainspecRegistry = BTreeMap<String, Digest>;

/// The entry in the `ChainspecRegistry` under which we store the hash of the chainspec.toml.
pub const CHAINSPEC_RAW: &str = "chainspec_raw";
/// The entry in the `ChainspecRegistry` under which we store the hash of the genesis_accounts.toml.
pub const GENESIS_ACCOUNTS_RAW: &str = "genesis_accounts_raw";
/// The entry in the `ChainspecRegistry` under which we store the hash of the global_state.toml.
pub const GLOBAL_STATE_RAW: &str = "global_state_raw";
