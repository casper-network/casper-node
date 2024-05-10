pub mod v2;

use std::sync::Arc;

use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::{tracking_copy::TrackingCopyParts, AddressGenerator};
use casper_types::{execution::Effects, Key, TransactionHash};
use parking_lot::RwLock;
use thiserror::Error;
use vm_common::selector::Selector;

use crate::{
    storage::{Address, GlobalStateReader, TrackingCopy},
    wasm_backend::{GasUsage, WasmPreparationError},
    HostError,
};

/// Request to execute a Wasm contract.
pub struct ExecuteRequest {
    /// Initiator's address.
    pub(crate) initiator: Address,
    /// Caller's address key.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub(crate) caller_key: Key,
    /// Callee's address key.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub(crate) callee_key: Key,
    /// Gas limit.
    pub(crate) gas_limit: u64,
    /// Target for execution.
    pub(crate) execution_kind: ExecutionKind,
    /// Input data.
    pub(crate) input: Bytes,
    /// Value transferred to the contract.
    pub(crate) value: u64,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of execution requests.
    pub(crate) address_generator: Arc<RwLock<AddressGenerator>>,
}

/// Builder for `ExecuteRequest`.
#[derive(Default)]
pub struct ExecuteRequestBuilder {
    initiator: Option<Address>,
    caller_key: Option<Key>,
    callee_key: Option<Key>,
    gas_limit: Option<u64>,
    target: Option<ExecutionKind>,
    input: Option<Bytes>,
    value: Option<u64>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
}

impl ExecuteRequestBuilder {
    /// Set the initiator's address.
    pub fn with_initiator(mut self, initiator: Address) -> Self {
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
    pub fn with_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    /// Set the address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of execution requests.
    pub fn with_address_generator(mut self, address_generator: AddressGenerator) -> Self {
        self.address_generator = Some(Arc::new(RwLock::new(address_generator)));
        self
    }

    /// Set the shared address generator.
    ///
    /// This is useful when the address generator is shared across a chain of multiple execution requests.
    pub fn with_shared_address_generator(
        mut self,
        address_generator: Arc<RwLock<AddressGenerator>>,
    ) -> Self {
        self.address_generator = Some(address_generator);
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
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct ExecuteResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub(crate) host_error: Option<HostError>,
    /// Output produced by the Wasm contract.
    pub(crate) output: Option<Bytes>,
    /// Gas usage.
    pub(crate) gas_usage: GasUsage,
    /// Effects produced by the execution.
    pub(crate) tracking_copy_parts: TrackingCopyParts,
}

impl ExecuteResult {
    /// Returns the host error.
    pub fn effects(&self) -> &Effects {
        let (_cache, effects, _x) = &self.tracking_copy_parts;
        effects
    }
}

/// Target for Wasm execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionKind {
    /// Execute Wasm bytes directly.
    WasmBytes(Bytes),
    /// Execute a stored contract by its address.
    Contract {
        /// Address of the contract.
        address: Address,
        /// Entry point selector.
        ///
        /// If `None`, fallback entry point is used (if present).
        selector: Option<Selector>,
    },
}

/// Error that can occur during execution, before the Wasm virtual machine is involved.
///
/// This error is returned by the `execute` function. It contains information about the error that occurred.
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
/// An executor is responsible for executing Wasm contracts. This implies that the executor is able to prepare Wasm instances, execute them, and handle errors that occur during execution.
///
/// Trait bounds also implying that the executor has to support interior mutability, as it may need to update its internal state during execution of a single or a chain of multiple contracts.
pub trait Executor: Clone + Send {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError>;
}
