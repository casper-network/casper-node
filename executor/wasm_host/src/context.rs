use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use bytes::Bytes;
use casper_executor_wasm_interface::executor::ExecutionKind;
use casper_storage::{
    global_state::GlobalStateReader, AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash, BlockTime, HostFFIFunctionCost, Key, MessageLimits, StorageCosts,
    TransactionHash, WasmV2Config,
};
use parking_lot::RwLock;

/// Container that holds all relevant modules necessary to process an execution request.
pub struct Context<S: GlobalStateReader> {
    /// The address of the account that initiated the contract or session code.
    pub initiator: AccountHash,
    /// The address of the addressable entity that is currently executing the contract or session
    /// code.
    pub caller: Key,
    /// The address of the addressable entity that is being called.
    pub callee: Key,
    /// The state of the global state at the time of the call based on the currently executing
    /// contract or session address.
    // pub state_address: Address,
    /// The amount of tokens that were send to the contract's purse at the time of the call.
    pub transferred_value: u64,
    pub config: WasmV2Config,
    pub storage_costs: StorageCosts,
    pub baseline_motes_amount: u64,
    pub message_limits: MessageLimits,
    pub tracking_copy: TrackingCopy<S>,
    pub transaction_hash: TransactionHash,
    pub address_generator: Arc<RwLock<AddressGenerator>>,
    pub chain_name: Arc<str>,
    pub input: Bytes,
    pub block_time: BlockTime,
    pub parent_block_hash: [u8; 32],
    pub block_height: u64,
    /// Whether the execution is in sandboxed mode.
    ///
    /// In sandboxed mode, the contract cannot make state changes, call other contracts, emit
    /// messages, etc. No gas is charged for the execution.
    pub sandboxed: bool,
    /// Runtime native config.
    pub runtime_native_config: RuntimeNativeConfig,
    /// Authorization keys for this execution.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// Map of ffi menu options to their respective cost entries
    pub ffi_call_costs: BTreeMap<u32, HostFFIFunctionCost>,
    /// Shared execution stack across nested calls
    pub execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
}
