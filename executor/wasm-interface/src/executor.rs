use std::sync::Arc;

use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::{
    global_state::GlobalStateReader, tracking_copy::TrackingCopyParts, AddressGenerator,
    TrackingCopy,
};
use casper_types::{account::AccountHash, execution::Effects, EntityAddr, Key, TransactionHash};
use parking_lot::RwLock;
use thiserror::Error;

use crate::{GasUsage, HostError, WasmPreparationError};

/// Request to execute a Wasm contract.
pub struct ExecuteRequest {
    /// Initiator's address.
    pub initiator: AccountHash,
    /// Caller's address key.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub caller_key: Key,
    /// Callee's address key. TODO: Remove this field as the callee is derived from the execution
    /// kind field.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub callee_key: Key,
    /// Gas limit.
    pub gas_limit: u64,
    /// Target for execution.
    pub execution_kind: ExecutionKind,
    /// Input data.
    pub input: Bytes,
    /// Value transferred to the contract.
    pub value: u128,
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
}

/// Builder for `ExecuteRequest`.
#[derive(Default)]
pub struct ExecuteRequestBuilder {
    initiator: Option<AccountHash>,
    caller_key: Option<Key>,
    callee_key: Option<Key>,
    gas_limit: Option<u64>,
    target: Option<ExecutionKind>,
    input: Option<Bytes>,
    value: Option<u128>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
    chain_name: Option<Arc<str>>,
}

impl ExecuteRequestBuilder {
    /// Set the initiator's address.
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    /// Set the caller's key.
    pub fn with_caller_key(mut self, caller_key: Key) -> Self {
        self.caller_key = Some(caller_key);
        self
    }

    /// Set the callee's key.
    pub fn with_callee_key(mut self, callee_key: Key) -> Self {
        self.callee_key = Some(callee_key);
        self
    }

    /// Set the gas limit.
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Set the target for execution.
    pub fn with_target(mut self, target: ExecutionKind) -> Self {
        self.target = Some(target);
        self
    }

    /// Pass input data.
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    /// Pass input data that can be serialized.
    pub fn with_serialized_input<T: BorshSerialize>(self, input: T) -> Self {
        let input = borsh::to_vec(&input)
            .map(Bytes::from)
            .expect("should serialize input");
        self.with_input(input)
    }

    /// Pass value to be sent to the contract.
    pub fn with_value(mut self, value: u128) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the transaction hash.
    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    /// Set the address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of
    /// execution requests.
    pub fn with_address_generator(mut self, address_generator: AddressGenerator) -> Self {
        self.address_generator = Some(Arc::new(RwLock::new(address_generator)));
        self
    }

    /// Set the shared address generator.
    ///
    /// This is useful when the address generator is shared across a chain of multiple execution
    /// requests.
    pub fn with_shared_address_generator(
        mut self,
        address_generator: Arc<RwLock<AddressGenerator>>,
    ) -> Self {
        self.address_generator = Some(address_generator);
        self
    }

    /// Set the chain name.
    pub fn with_chain_name<T: Into<Arc<str>>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Build the `ExecuteRequest`.
    pub fn build(self) -> Result<ExecuteRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let caller_key = self.caller_key.ok_or("Caller is not set")?;
        let callee_key = self.callee_key.ok_or("Callee is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let execution_kind = self.target.ok_or("Target is not set")?;
        let input = self.input.ok_or("Input is not set")?;
        let value = self.value.ok_or("Value is not set")?;
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash is not set")?;
        let address_generator = self
            .address_generator
            .ok_or("Address generator is not set")?;
        let chain_name = self.chain_name.ok_or("Chain name is not set")?;
        Ok(ExecuteRequest {
            initiator,
            caller_key,
            callee_key,
            gas_limit,
            execution_kind,
            input,
            value,
            transaction_hash,
            address_generator,
            chain_name,
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct ExecuteResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub host_error: Option<HostError>,
    /// Output produced by the Wasm contract.
    pub output: Option<Bytes>,
    /// Gas usage.
    pub gas_usage: GasUsage,
    /// Effects produced by the execution.
    pub tracking_copy_parts: TrackingCopyParts,
}

impl ExecuteResult {
    /// Returns the host error.
    pub fn effects(&self) -> &Effects {
        let (_cache, effects, _x) = &self.tracking_copy_parts;
        effects
    }

    pub fn host_error(&self) -> Option<&HostError> {
        self.host_error.as_ref()
    }

    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
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
        address: EntityAddr,
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
}
