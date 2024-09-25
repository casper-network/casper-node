use std::sync::Arc;

use bytes::Bytes;
use casper_executor_wasm_interface::executor::Executor;
use casper_storage::{global_state::GlobalStateReader, AddressGenerator, TrackingCopy};
use casper_types::{account::AccountHash, Key, Timestamp, TransactionHash};
use parking_lot::RwLock;

/// Container that holds all relevant modules necessary to process an execution request.
pub struct Context<S: GlobalStateReader, E: Executor> {
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
    pub transferred_value: u128,
    pub tracking_copy: TrackingCopy<S>,
    pub executor: E, // TODO: This could be part of the caller
    pub transaction_hash: TransactionHash,
    pub address_generator: Arc<RwLock<AddressGenerator>>,
    pub chain_name: Arc<str>,
    pub input: Bytes,
    pub block_time: Timestamp,
}
