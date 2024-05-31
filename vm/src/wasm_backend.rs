pub(crate) mod wasmer;
use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use casper_storage::AddressGenerator;
use casper_types::{system, AddressableEntity, Key, TransactionHash, URef};
use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    executor::ExecutionKind,
    storage::{Address, GlobalStateReader, TrackingCopy},
    Config, Executor, VMError, VMResult,
};

#[derive(Debug)]
pub struct GasUsage {
    /// The amount of gas used by the execution.
    pub(crate) gas_limit: u64,
    /// The amount of gas remaining after the execution.
    pub(crate) remaining_points: u64,
}

impl GasUsage {
    pub fn new(gas_limit: u64, remaining_points: u64) -> Self {
        GasUsage {
            gas_limit,
            remaining_points,
        }
    }

    pub fn gas_spent(&self) -> u64 {
        debug_assert!(self.remaining_points <= self.gas_limit);
        self.gas_limit - self.remaining_points
    }
}

/// Container that holds all relevant modules necessary to process an execution request.
pub(crate) struct Context<S: GlobalStateReader, E: Executor> {
    /// The address of the account that initiated the contract or session code.
    pub(crate) initiator: Address,
    /// The address of the addressable entity that is currently executing the contract or session code.
    pub(crate) caller: Key,
    /// The address of the addressable entity that is being called.
    pub(crate) callee: Key,
    /// The state of the global state at the time of the call based on the currently executing contract or session address.
    // pub(crate) state_address: Address,
    /// The amount of tokens that were send to the contract's purse at the time of the call.
    pub(crate) value: u64,
    pub(crate) tracking_copy: TrackingCopy<S>,
    pub(crate) executor: E, // TODO: This could be part of the caller
    pub(crate) transaction_hash: TransactionHash,
    pub(crate) address_generator: Arc<RwLock<AddressGenerator>>,
}

#[derive(Debug, Copy, Clone)]
pub enum MeteringPoints {
    Remaining(u64),
    Exhausted,
}

impl MeteringPoints {
    pub fn try_into_remaining(self) -> Result<u64, Self> {
        if let Self::Remaining(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

/// An abstraction over the 'caller' object of a host function that works for any Wasm VM.
///
/// This allows access for important instances such as the context object that was passed to the
/// instance, wasm linear memory access, etc.

pub(crate) trait Caller<S: GlobalStateReader, E: Executor> {
    fn config(&self) -> &Config;
    fn context(&self) -> &Context<S, E>;
    fn context_mut(&mut self) -> &mut Context<S, E>;
    /// Returns currently running *unmodified* bytecode.
    fn bytecode(&self) -> Bytes;

    fn memory_read(&self, offset: u32, size: usize) -> VMResult<Vec<u8>> {
        let mut vec = vec![0; size];
        self.memory_read_into(offset, &mut vec)?;
        Ok(vec)
    }
    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> VMResult<()>;
    fn memory_write(&self, offset: u32, data: &[u8]) -> VMResult<()>;
    /// Allocates memory inside the Wasm VM by calling an export.
    ///
    /// Error is a type-erased error coming from the VM itself.
    fn alloc(&mut self, idx: u32, size: usize, ctx: u32) -> VMResult<u32>;
    /// Returns the amount of gas used.
    fn gas_consumed(&mut self) -> MeteringPoints;
    /// Set the amount of gas used.
    fn consume_gas(&mut self, value: u64) -> MeteringPoints;
}

#[derive(Debug, Error)]
pub enum WasmPreparationError {
    #[error("Missing export {0}")]
    MissingExport(String),
    #[error("Compile error: {0}")]
    Compile(String),
    #[error("Memory instantiation error: {0}")]
    Memory(String),
    #[error("Instantiation error: {0}")]
    Instantiation(String),
}

pub(crate) trait WasmInstance<S: GlobalStateReader, E: Executor> {
    fn call_export(&mut self, name: &str) -> (Result<(), VMError>, GasUsage);
    fn call_function(&mut self, function_index: u32) -> (Result<(), VMError>, GasUsage);
    fn teardown(self) -> Context<S, E>;
}
