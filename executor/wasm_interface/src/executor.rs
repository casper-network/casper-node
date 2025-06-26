use std::sync::Arc;

use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::{
    global_state::{error::Error as GlobalStateError, GlobalStateReader},
    tracking_copy::TrackingCopyCache,
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash, contract_messages::Messages, execution::Effects, BlockHash, BlockTime,
    Digest, HashAddr, Key, TransactionHash, bytesrepr::{self, FromBytes, ToBytes},
};
use parking_lot::RwLock;
use thiserror::Error;

use crate::{CallError, GasUsage, InternalHostError, WasmPreparationError};

/// Request to execute a Wasm contract.
pub struct ExecuteRequest {
    /// Initiator's address.
    pub initiator: AccountHash,
    /// Caller's address key.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub caller_key: Key,
    /// Gas limit.
    pub gas_limit: u64,
    /// Target for execution.
    pub execution_kind: ExecutionKind,
    /// Input data.
    pub input: Bytes,
    /// Value transferred to the contract.
    pub transferred_value: u64,
    /// Transaction hash.
    pub transaction_hash: TransactionHash,
    /// Address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of
    /// execution requests.
    pub address_generator: Arc<RwLock<AddressGenerator>>,
    /// Chain name.
    ///
    /// This is very important ingredient for deriving contract hashes on the network.
    pub chain_name: Arc<str>,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// State root hash of the global state in which the transaction will be executed.
    pub state_hash: Digest,
    /// Parent block hash.
    pub parent_block_hash: BlockHash,
    /// Block height.
    pub block_height: u64,
    /// Whether the execution is in read-only mode.
    ///
    /// In read-only mode, the contract cannot make any state changes (writes, transfers, etc.)
    /// and no gas is charged for the execution.
    pub read_only: bool,
}

/// Builder for `ExecuteRequest`.
#[derive(Default)]
pub struct ExecuteRequestBuilder {
    initiator: Option<AccountHash>,
    caller_key: Option<Key>,
    gas_limit: Option<u64>,
    target: Option<ExecutionKind>,
    input: Option<Bytes>,
    value: Option<u64>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
    chain_name: Option<Arc<str>>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    read_only: Option<bool>,
}

impl ExecuteRequestBuilder {
    /// Set the initiator's address.
    #[must_use]
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    /// Set the caller's key.
    #[must_use]
    pub fn with_caller_key(mut self, caller_key: Key) -> Self {
        self.caller_key = Some(caller_key);
        self
    }

    /// Set the gas limit.
    #[must_use]
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Set the target for execution.
    #[must_use]
    pub fn with_target(mut self, target: ExecutionKind) -> Self {
        self.target = Some(target);
        self
    }

    /// Pass input data.
    #[must_use]
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    /// Pass input data that can be serialized.
    #[must_use]
    pub fn with_serialized_input<T: BorshSerialize>(self, input: T) -> Self {
        let input = borsh::to_vec(&input)
            .map(Bytes::from)
            .expect("should serialize input");
        self.with_input(input)
    }

    /// Pass value to be sent to the contract.
    #[must_use]
    pub fn with_transferred_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the transaction hash.
    #[must_use]
    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    /// Set the address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of
    /// execution requests.
    #[must_use]
    pub fn with_address_generator(mut self, address_generator: AddressGenerator) -> Self {
        self.address_generator = Some(Arc::new(RwLock::new(address_generator)));
        self
    }

    /// Set the shared address generator.
    ///
    /// This is useful when the address generator is shared across a chain of multiple execution
    /// requests.
    #[must_use]
    pub fn with_shared_address_generator(
        mut self,
        address_generator: Arc<RwLock<AddressGenerator>>,
    ) -> Self {
        self.address_generator = Some(address_generator);
        self
    }

    /// Set the chain name.
    #[must_use]
    pub fn with_chain_name<T: Into<Arc<str>>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Set the block time.
    #[must_use]
    pub fn with_block_time(mut self, block_time: BlockTime) -> Self {
        self.block_time = Some(block_time);
        self
    }

    /// Set the state hash.
    #[must_use]
    pub fn with_state_hash(mut self, state_hash: Digest) -> Self {
        self.state_hash = Some(state_hash);
        self
    }

    /// Set the parent block hash.
    #[must_use]
    pub fn with_parent_block_hash(mut self, parent_block_hash: BlockHash) -> Self {
        self.parent_block_hash = Some(parent_block_hash);
        self
    }

    /// Set the block height.
    #[must_use]
    pub fn with_block_height(mut self, block_height: u64) -> Self {
        self.block_height = Some(block_height);
        self
    }

    /// Set the read-only mode.
    #[must_use]
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = Some(read_only);
        self
    }

    /// Build the `ExecuteRequest`.
    pub fn build(self) -> Result<ExecuteRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let caller_key = self.caller_key.ok_or("Caller is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let execution_kind = self.target.ok_or("Target is not set")?;
        let input = self.input.ok_or("Input is not set")?;
        let transferred_value = self.value.ok_or("Value is not set")?;
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash is not set")?;
        let address_generator = self
            .address_generator
            .ok_or("Address generator is not set")?;
        let chain_name = self.chain_name.ok_or("Chain name is not set")?;
        let block_time = self.block_time.ok_or("Block time is not set")?;
        let state_hash = self.state_hash.ok_or("State hash is not set")?;
        let parent_block_hash = self
            .parent_block_hash
            .ok_or("Parent block hash is not set")?;
        let block_height = self.block_height.ok_or("Block height is not set")?;
        let read_only = self.read_only.unwrap_or(false);
        Ok(ExecuteRequest {
            initiator,
            caller_key,
            gas_limit,
            execution_kind,
            input,
            transferred_value,
            transaction_hash,
            address_generator,
            chain_name,
            block_time,
            state_hash,
            parent_block_hash,
            block_height,
            read_only,
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct ExecuteResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub host_error: Option<CallError>,
    /// Output produced by the Wasm contract.
    pub output: Option<Bytes>,
    /// Gas usage.
    pub gas_usage: GasUsage,
    /// Effects produced by the execution.
    pub effects: Effects,
    /// Cache of tracking copy effects produced by the execution.
    pub cache: TrackingCopyCache,
    /// Messages produced by the execution.
    pub messages: Messages,
}

impl ExecuteResult {
    /// Returns the host error.
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    pub fn into_effects(self) -> Effects {
        self.effects
    }

    pub fn host_error(&self) -> Option<&CallError> {
        self.host_error.as_ref()
    }

    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }
}

/// Result of executing a Wasm contract on a state provider.
#[derive(Debug)]
pub struct ExecuteWithProviderResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub host_error: Option<CallError>,
    /// Output produced by the Wasm contract.
    output: Option<Bytes>,
    /// Gas usage.
    gas_usage: GasUsage,
    /// Effects produced by the execution.
    effects: Effects,
    /// Post state hash.
    post_state_hash: Digest,
    /// Messages produced by the execution.
    messages: Messages,
}

impl ExecuteWithProviderResult {
    #[must_use]
    pub fn new(
        host_error: Option<CallError>,
        output: Option<Bytes>,
        gas_usage: GasUsage,
        effects: Effects,
        post_state_hash: Digest,
        messages: Messages,
    ) -> Self {
        Self {
            host_error,
            output,
            gas_usage,
            effects,
            post_state_hash,
            messages,
        }
    }

    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }

    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    #[must_use]
    pub fn post_state_hash(&self) -> Digest {
        self.post_state_hash
    }

    pub fn messages(&self) -> &Messages {
        &self.messages
    }
}

/// Target for Wasm execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionKind {
    /// Execute Wasm bytes directly.
    SessionBytes(Bytes),
    /// Execute a stored contract by its address.
    Stored {
        /// Address of the contract.
        address: HashAddr,
        /// Entry point to call.
        entry_point: String,
    },
}

/// Error that can occur during execution, before the Wasm virtual machine is involved.
///
/// This error is returned by the `execute` function. It contains information about the error that
/// occurred.
#[derive(Debug, Error)]
pub enum ExecuteError {
    /// Error while preparing Wasm instance: export not found, validation, compilation errors, etc.
    ///
    /// No wasm was executed at this point.
    #[error("Wasm error error: {0}")]
    WasmPreparation(#[from] WasmPreparationError),
    /// Error while executing Wasm: traps, memory access errors, etc.
    #[error("Internal host error: {0}")]
    InternalHost(#[from] InternalHostError),
    #[error("Code not found")]
    CodeNotFound(HashAddr),
}

#[derive(Debug, Error)]
pub enum ExecuteWithProviderError {
    /// Error while accessing global state.
    #[error("Global state error: {0}")]
    GlobalState(#[from] GlobalStateError),
    #[error(transparent)]
    Execute(#[from] ExecuteError),
}

/// Executor trait.
///
/// An executor is responsible for executing Wasm contracts. This implies that the executor is able
/// to prepare Wasm instances, execute them, and handle errors that occur during execution.
///
/// Trait bounds also implying that the executor has to support interior mutability, as it may need
/// to update its internal state during execution of a single or a chain of multiple contracts.
pub trait Executor: Clone + Send {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError>;

    /// Execute a read-only query on a contract.
    ///
    /// This method executes a contract in read-only mode without making any state changes
    /// or broadcasting the transaction.
    fn query<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        query_request: QueryRequest,
    ) -> Result<QueryResult, ExecuteError>;
}

/// A request to execute a read-only query on a contract.
///
/// This allows off-chain querying of contract state without making global state changes
/// or costing gas. A gas limit must be provided to prevent infinite loops and resource
/// exhaustion attacks.
#[derive(Debug, PartialEq)]
pub struct QueryRequest {
    /// The address of the account that would initiate the contract call.
    pub initiator: AccountHash,
    /// The address of the contract to query.
    pub contract_address: HashAddr,
    /// The entry point to call.
    pub entry_point: String,
    /// Input data for the query.
    pub input: Vec<u8>,
    /// Gas limit for the query execution.
    /// 
    /// This prevents infinite loops and resource exhaustion attacks.
    /// The caller is not charged actual tokens, but must provide a limit
    /// to protect against malicious contracts that could stall the node.
    pub gas_limit: u64,
    /// Block time for the query context.
    pub block_time: BlockTime,
    /// State root hash to query against.
    pub state_hash: Digest,
    /// Parent block hash for context.
    pub parent_block_hash: BlockHash,
    /// Block height for context.
    pub block_height: u64,
    /// Chain name for context.
    pub chain_name: String,
}

impl ToBytes for QueryRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut writer = bytesrepr::allocate_buffer(self)?;
        self.state_hash.write_bytes(&mut writer)?;
        self.contract_address.write_bytes(&mut writer)?;
        self.entry_point.write_bytes(&mut writer)?;
        self.input.write_bytes(&mut writer)?;
        self.gas_limit.write_bytes(&mut writer)?;
        Ok(writer)
    }

    fn serialized_length(&self) -> usize {
        self.state_hash.serialized_length()
            + self.contract_address.serialized_length()
            + self.entry_point.serialized_length()
            + self.input.serialized_length()
            + self.gas_limit.serialized_length()
    }
}

impl FromBytes for QueryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (state_hash, remainder) = FromBytes::from_bytes(bytes)?;
        let (contract_address, remainder) = FromBytes::from_bytes(remainder)?;
        let (entry_point, remainder) = FromBytes::from_bytes(remainder)?;
        let (input, remainder) = FromBytes::from_bytes(remainder)?;
        let (gas_limit, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            QueryRequest {
                initiator: AccountHash::default(),
                contract_address,
                entry_point,
                input,
                gas_limit,
                block_time: BlockTime::default(),
                state_hash,
                parent_block_hash: BlockHash::default(),
                block_height: 0,
                chain_name: String::default(),
            },
            remainder,
        ))
    }
}

#[cfg(test)]
impl QueryRequest {
    pub(crate) fn random(rng: &mut casper_types::testing::TestRng) -> Self {
        use casper_types::testing::TestRng;
        use rand::Rng;
        
        QueryRequest {
            initiator: AccountHash::random(rng),
            contract_address: HashAddr::random(rng),
            entry_point: format!("entry_point_{}", rng.gen::<u32>()),
            input: vec![rng.gen::<u8>(); 32],
            gas_limit: rng.gen_range(1000..1000000),
            block_time: BlockTime::new(rng.gen()),
            state_hash: Digest::random(rng),
            parent_block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
            chain_name: String::default(),
        }
    }
}

/// Result of executing a read-only query.
#[derive(Debug)]
pub struct QueryResult {
    /// Error while executing the query, if any.
    pub error: Option<CallError>,
    /// Output data returned by the contract.
    pub output: Option<Bytes>,
    /// Gas usage tracked during execution. Use `gas_spent()` to get the gas consumed.
    pub gas_usage: GasUsage,
}

impl QueryResult {
    /// Returns the error if the query failed.
    pub fn error(&self) -> Option<&CallError> {
        self.error.as_ref()
    }

    /// Returns the output data if the query succeeded.
    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    /// Returns the gas usage.
    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }

    /// Returns the gas spent.
    pub fn gas_spent(&self) -> u64 {
        self.gas_usage.gas_spent()
    }

    /// Returns true if the query was successful.
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// Builder for `QueryRequest`.
#[derive(Default)]
pub struct QueryRequestBuilder {
    initiator: Option<AccountHash>,
    contract_address: Option<HashAddr>,
    entry_point: Option<String>,
    input: Option<Vec<u8>>,
    gas_limit: Option<u64>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    chain_name: Option<String>,
}

impl QueryRequestBuilder {
    /// Set the initiator's address.
    #[must_use]
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    /// Set the contract address to query.
    #[must_use]
    pub fn with_contract_address(mut self, contract_address: HashAddr) -> Self {
        self.contract_address = Some(contract_address);
        self
    }

    /// Set the entry point to call.
    #[must_use]
    pub fn with_entry_point(mut self, entry_point: String) -> Self {
        self.entry_point = Some(entry_point);
        self
    }

    /// Set the input data.
    #[must_use]
    pub fn with_input(mut self, input: Vec<u8>) -> Self {
        self.input = Some(input);
        self
    }

    /// Set the input data that can be serialized.
    #[must_use]
    pub fn with_serialized_input<T: BorshSerialize>(self, input: T) -> Self {
        let input = borsh::to_vec(&input)
            .map(Vec::from)
            .expect("should serialize input");
        self.with_input(input)
    }

    /// Set the gas limit.
    #[must_use]
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Set the block time.
    #[must_use]
    pub fn with_block_time(mut self, block_time: BlockTime) -> Self {
        self.block_time = Some(block_time);
        self
    }

    /// Set the state hash.
    #[must_use]
    pub fn with_state_hash(mut self, state_hash: Digest) -> Self {
        self.state_hash = Some(state_hash);
        self
    }

    /// Set the parent block hash.
    #[must_use]
    pub fn with_parent_block_hash(mut self, parent_block_hash: BlockHash) -> Self {
        self.parent_block_hash = Some(parent_block_hash);
        self
    }

    /// Set the block height.
    #[must_use]
    pub fn with_block_height(mut self, block_height: u64) -> Self {
        self.block_height = Some(block_height);
        self
    }

    /// Set the chain name.
    #[must_use]
    pub fn with_chain_name<T: Into<String>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Build the `QueryRequest`.
    pub fn build(self) -> Result<QueryRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let contract_address = self.contract_address.ok_or("Contract address is not set")?;
        let entry_point = self.entry_point.ok_or("Entry point is not set")?;
        let input = self.input.ok_or("Input is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let block_time = self.block_time.ok_or("Block time is not set")?;
        let state_hash = self.state_hash.ok_or("State hash is not set")?;
        let parent_block_hash = self.parent_block_hash.ok_or("Parent block hash is not set")?;
        let block_height = self.block_height.ok_or("Block height is not set")?;
        let chain_name = self.chain_name.ok_or("Chain name is not set")?;
        Ok(QueryRequest {
            initiator,
            contract_address,
            entry_point,
            input,
            gas_limit,
            block_time,
            state_hash,
            parent_block_hash,
            block_height,
            chain_name,
        })
    }
}
