pub mod v2;

use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::{tracking_copy::TrackingCopyParts, TrackingCopy};
use casper_types::execution::Effects;
use thiserror::Error;
use vm_common::selector::Selector;

use crate::{
    storage::{Address, GlobalStateReader},
    wasm_backend::{GasUsage, PreparationError},
    HostError,
};

/// Request to execute a Wasm contract.
pub struct ExecuteRequest {
    /// Caller's address.
    caller: Address,
    /// Address of the contract to execute.
    address: Address,
    /// Gas limit.
    gas_limit: u64,
    execution_kind: ExecutionKind,
    input: Bytes,
}

/// Builder for `ExecuteRequest`.
#[derive(Default)]
pub struct ExecuteRequestBuilder {
    caller: Option<Address>,
    address: Option<Address>,
    gas_limit: Option<u64>,
    target: Option<ExecutionKind>,
    input: Option<Bytes>,
}

impl ExecuteRequestBuilder {
    /// Set the caller's address.
    pub fn with_caller(mut self, caller: Address) -> Self {
        self.caller = Some(caller);
        self
    }

    /// Set the address of the contract to execute.
    pub fn with_address(mut self, address: Address) -> Self {
        self.address = Some(address);
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

    /// Build the `ExecuteRequest`.
    pub fn build(self) -> Result<ExecuteRequest, &'static str> {
        let caller = self.caller.ok_or("Caller is not set")?;
        let address = self.address.ok_or("Address is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let execution_kind = self.target.ok_or("Target is not set")?;
        let input = self.input.ok_or("Input is not set")?;

        Ok(ExecuteRequest {
            caller,
            address,
            gas_limit,
            execution_kind,
            input,
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
pub enum ExecutionKind {
    /// Execute Wasm bytes directly.
    WasmBytes(Bytes),
    /// Execute a stored contract by its address.
    Contract {
        address: Address,
        selector: Selector,
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
    Preparation(#[from] PreparationError),
}

pub trait Executor: Clone + Send {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError>;
}
