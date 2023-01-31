//! Support for runtime configuration of the execution engine - as an integral property of the
//! `EngineState` instance.
use casper_types::{
    bytesrepr::{U64_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    KEY_HASH_LENGTH,
};

use crate::shared::{system_config::SystemConfig, wasm_config::WasmConfig};

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;
/// Default value for maximum StoredValue serialized size configuration option.
pub const DEFAULT_MAX_STORED_VALUE_SIZE: u32 = 8 * 1024 * 1024;
/// Default value for maximum delegators per validator.
pub const DEFAULT_MAX_DELEGATOR_SIZE_LIMIT: u32 = 950;
/// Default value for minimum delegation amount in motes.
pub const DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64 = 500 * 1_000_000_000;

/// The size in bytes per delegator entry in SeigniorageRecipient struct.
/// 34 bytes PublicKey + 10 bytes U512 for the delegated amount
const SIZE_PER_DELEGATOR_ENTRY: u32 = 44;
/// The fixed portion, in bytes, of the SeigniorageRecipient struct and its key in the
/// `SeigniorageRecipients` map.
/// 34 bytes for the public key + 10 bytes validator weight + 1 byte for delegation rate.
const FIXED_SIZE_PER_VALIDATOR: u32 = 45;
/// The size of a key of the `SeigniorageRecipientsSnapshot`, i.e. an `EraId`.
const FIXED_SIZE_PER_ERA: u32 = U64_SERIALIZED_LENGTH as u32;
/// The overhead of the Key::Hash under which the seigniorage snapshot lives.
/// The hash length plus an additional byte for the tag.
const KEY_HASH_SERIALIZED_LENGTH: u32 = KEY_HASH_LENGTH as u32 + 1;

#[doc(hidden)]
pub const fn compute_max_delegator_size_limit(
    max_stored_value_size: u32,
    auction_delay: u64,
    validator_slots: u32,
) -> u32 {
    let size_limit_per_snapshot =
        (max_stored_value_size - U8_SERIALIZED_LENGTH as u32 - KEY_HASH_SERIALIZED_LENGTH)
            / (auction_delay + 1) as u32;
    let size_per_seigniorage_recipients = size_limit_per_snapshot - FIXED_SIZE_PER_ERA;
    let size_limit_per_validator =
        (size_per_seigniorage_recipients / validator_slots) - FIXED_SIZE_PER_VALIDATOR;
    // The max number of the delegators per validator is the size limit allotted
    // to a single validator divided by the size of a single delegator entry.
    // For the given:
    // 1. max limit of 8MB
    // 2. 100 validator slots
    // 3. an auction delay of 1
    // There will be a maximum of roughly 953 delegators per validator.
    size_limit_per_validator / SIZE_PER_DELEGATOR_ENTRY
}

/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone)]
pub struct EngineConfig {
    /// Max query depth of the engine.
    pub(crate) max_query_depth: u64,
    /// Maximum number of associated keys (i.e. map of
    /// [`AccountHash`](casper_types::account::AccountHash)s to
    /// [`Weight`](casper_types::account::Weight)s) for a single account.
    max_associated_keys: u32,
    max_runtime_call_stack_height: u32,
    max_stored_value_size: u32,
    max_delegator_size_limit: u32,
    minimum_delegation_amount: u64,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
            max_associated_keys: DEFAULT_MAX_ASSOCIATED_KEYS,
            max_runtime_call_stack_height: DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
            max_stored_value_size: DEFAULT_MAX_STORED_VALUE_SIZE,
            max_delegator_size_limit: DEFAULT_MAX_DELEGATOR_SIZE_LIMIT,
            minimum_delegation_amount: DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
        }
    }
}

impl EngineConfig {
    /// Creates a new engine configuration with provided parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_query_depth: u64,
        max_associated_keys: u32,
        max_runtime_call_stack_height: u32,
        max_stored_value_size: u32,
        max_delegator_size_limit: u32,
        minimum_delegation_amount: u64,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
    ) -> EngineConfig {
        EngineConfig {
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            max_stored_value_size,
            max_delegator_size_limit,
            minimum_delegation_amount,
            wasm_config,
            system_config,
        }
    }

    /// Returns the current max associated keys config.
    pub fn max_associated_keys(&self) -> u32 {
        self.max_associated_keys
    }

    /// Returns the current max runtime call stack height config.
    pub fn max_runtime_call_stack_height(&self) -> u32 {
        self.max_runtime_call_stack_height
    }

    /// Returns the current max runtime call stack height config.
    pub fn max_stored_value_size(&self) -> u32 {
        self.max_stored_value_size
    }

    /// Returns the current maximum of delegators per validator.
    pub fn max_delegator_size_limit(&self) -> u32 {
        self.max_delegator_size_limit
    }

    /// Returns the current wasm config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns the current system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }

    /// Returns the minimum delegation amount in motes.
    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }
}
