pub mod chain;
pub(crate) mod host;
pub mod storage;
pub(crate) mod wasm_backend;
use std::sync::Arc;

use bytes::Bytes;

use casper_storage::{
    global_state::{self, state::StateReader},
    tracking_copy::{TrackingCopyCache, TrackingCopyParts},
};
use casper_types::{
    contract_messages::Messages, contracts::ContractV2, execution::Effects, ByteCodeAddr, Digest,
    EntityAddr, Key, StoredValue,
};
use either::Either;
use storage::{Address, GlobalStateReader, TrackingCopy};
use thiserror::Error;
use vm_common::{flags::ReturnFlags, selector::Selector};
use wasm_backend::{wasmer::WasmerInstance, Context, GasUsage, PreparationError, WasmInstance};

const CALLEE_SUCCEED: u32 = 0;
const CALLEE_REVERTED: u32 = 1;
const CALLEE_TRAPPED: u32 = 2;
const CALLEE_GAS_DEPLETED: u32 = 3;

/// Represents the result of a host function call.
///
/// 0 is used as a success.
#[derive(Debug)]
#[repr(u32)]
pub enum HostError {
    /// Callee contract reverted.
    CalleeReverted,
    /// Called contract trapped.
    CalleeTrapped(TrapCode),
    /// Called contract reached gas limit.
    CalleeGasDepleted,
}

// no revert: output
// revert: output
// trap: no output
// gas depleted: no output

type HostResult = Result<(), HostError>;

impl HostError {
    pub(crate) fn into_u32(self) -> u32 {
        match self {
            HostError::CalleeReverted => CALLEE_REVERTED,
            HostError::CalleeTrapped(_) => CALLEE_TRAPPED,
            HostError::CalleeGasDepleted => CALLEE_GAS_DEPLETED,
        }
    }
}

pub(crate) fn u32_from_host_result(result: HostResult) -> u32 {
    match result {
        Ok(_) => CALLEE_SUCCEED,
        Err(host_error) => host_error.into_u32(),
    }
}

/// `WasmEngine` is a struct that represents a WebAssembly engine.
///
/// This struct is used to encapsulate the operations and state of a WebAssembly engine.
/// Currently, it does not hold any data (`()`), but it can be extended in the future to hold
/// different compiler instances, configuration options, cached artifacts, state information,
/// etc.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// let engine = WasmEngine::new();
/// ```
#[derive(Clone)]
pub struct WasmEngine(());

#[derive(Debug, Error)]
pub enum Resolver {
    #[error("export {name} not found.")]
    Export { name: String },
    /// Trying to call a function pointer by index.
    #[error("function pointer {index} not found.")]
    Table { index: u32 },
}

#[derive(Error, Debug)]
pub enum ExportError {
    /// An error than occurs when the exported type and the expected type
    /// are incompatible.
    #[error("Incompatible Export Type")]
    IncompatibleType,
    /// This error arises when an export is missing
    #[error("Missing export {0}")]
    Missing(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum MemoryError {
    /// Memory access is outside heap bounds.
    #[error("memory access out of bounds")]
    HeapOutOfBounds,
    /// Address calculation overflow.
    #[error("address calculation overflow")]
    Overflow,
    /// String is not valid UTF-8.
    #[error("string is not valid utf-8")]
    NonUtf8String,
}

#[derive(Debug, Error)]
pub enum TrapCode {
    #[error("call stack exhausted")]
    StackOverflow,
    #[error("out of bounds memory access")]
    MemoryOutOfBounds,
    #[error("undefined element: out of bounds table access")]
    TableAccessOutOfBounds,
    #[error("uninitialized element")]
    IndirectCallToNull,
    #[error("indirect call type mismatch")]
    BadSignature,
    #[error("integer overflow")]
    IntegerOverflow,
    #[error("integer divide by zero")]
    IntegerDivisionByZero,
    #[error("invalid conversion to integer")]
    BadConversionToInteger,
    #[error("unreachable")]
    UnreachableCodeReached,
}

/// The outcome of a call.
/// We can fold all errors into this type and return it from the host functions and remove Outcome
/// type.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VMError {
    #[error("Return 0x{flags:?} {data:?}")]
    Return {
        flags: ReturnFlags,
        data: Option<Bytes>,
    },
    #[error("Out of gas")]
    OutOfGas,
    /// Error while executing Wasm: traps, memory access errors, etc.
    ///
    /// NOTE: for supporting multiple different backends we may want to abstract this a bit and
    /// extract memory access errors, trap codes, and unify error reporting.
    #[error("Trap: {0}")]
    Trap(TrapCode),
}

impl VMError {
    pub fn into_output_data(self) -> Option<Bytes> {
        match self {
            VMError::Return { data, .. } => data,
            _ => None,
        }
    }
}

pub type VMResult<T> = Result<T, VMError>;

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) gas_limit: u64,
    pub(crate) memory_limit: u32,
    pub(crate) input: Bytes,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigBuilder {
    gas_limit: Option<u64>,
    /// Memory limit in pages.
    memory_limit: Option<u32>,
    /// Input data.
    input: Option<Bytes>,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Memory limit denominated in pages.
    pub fn with_memory_limit(mut self, memory_limit: u32) -> Self {
        self.memory_limit = Some(memory_limit);
        self
    }

    /// Pass input data.
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    pub fn build(self) -> Config {
        let gas_limit = self.gas_limit.expect("Required field");
        let memory_limit = self.memory_limit.expect("Required field");
        let input = self.input.unwrap_or_default();
        Config {
            gas_limit,
            memory_limit,
            input,
        }
    }
}

impl WasmEngine {
    pub fn prepare<S: GlobalStateReader + 'static, E: Executor + 'static, C: Into<Bytes>>(
        &mut self,
        wasm_bytes: C,
        context: Context<S, E>,
        config: Config,
    ) -> Result<impl WasmInstance<S, E>, PreparationError> {
        let wasm_bytes: Bytes = wasm_bytes.into();
        let instance = WasmerInstance::from_wasm_bytes(wasm_bytes, context, config)?;
        Ok(instance)
    }

    pub fn new() -> Self {
        WasmEngine(())
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ExecutorKind {
    /// Ahead of time compiled Wasm.
    ///
    /// This is the default executor kind.
    Compiled,
}

#[derive(Copy, Clone, Debug)]
pub struct ExecutorConfig {
    memory_limit: u32,
    executor_kind: ExecutorKind,
}

impl ExecutorConfig {
    pub fn new() -> ExecutorConfigBuilder {
        ExecutorConfigBuilder::default()
    }
}

#[derive(Default)]
pub struct ExecutorConfigBuilder {
    memory_limit: Option<u32>,
    executor_kind: Option<ExecutorKind>,
}

impl ExecutorConfigBuilder {
    /// Set the memory limit.
    pub fn with_memory_limit(mut self, memory_limit: u32) -> Self {
        self.memory_limit = Some(memory_limit);
        self
    }

    /// Set the executor kind.
    pub fn with_executor_kind(mut self, executor_kind: ExecutorKind) -> Self {
        self.executor_kind = Some(executor_kind);
        self
    }

    /// Build the `ExecutorConfig`.
    pub fn build(self) -> Result<ExecutorConfig, &'static str> {
        let memory_limit = self.memory_limit.ok_or("Memory limit is not set")?;
        let executor_kind = self.executor_kind.ok_or("Executor kind is not set")?;

        Ok(ExecutorConfig {
            memory_limit,
            executor_kind,
        })
    }
}

#[derive(Clone)]
pub struct ExecutorV2 {
    config: ExecutorConfig,
    vm: Arc<WasmEngine>,
}

pub trait Executor: Clone + Send {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError>;
}

const DEFAULT_WASM_ENTRY_POINT: &str = "call";

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
    host_error: Option<HostError>,
    /// Output produced by the Wasm contract.
    output: Option<Bytes>,
    /// Gas usage.
    gas_usage: GasUsage,
    /// Effects produced by the execution.
    tracking_copy_parts: TrackingCopyParts,
}

impl ExecuteResult {
    /// Returns the host error.
    pub fn effects(&self) -> &Effects {
        let (_cache, effects, _) = &self.tracking_copy_parts;
        effects
    }
}

impl ExecutorV2 {
    /// Create a new `ExecutorV2` instance.
    pub fn new(config: ExecutorConfig) -> Self {
        let vm = WasmEngine::new();
        ExecutorV2 {
            config,
            vm: Arc::new(vm),
        }
    }
}

impl Executor for ExecutorV2 {
    /// Execute a Wasm contract.
    ///
    /// # Errors
    /// Returns an error if the execution fails. This can happen if the Wasm instance cannot be prepared.
    /// Otherwise, returns the result of the execution with a gas usage attached which means a successful execution (that may or may not have produced an error such as a trap, return, or out of gas).
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        mut tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        let ExecuteRequest {
            address,
            gas_limit,
            execution_kind: target,
            input,
            caller,
        } = execute_request;

        let (wasm_bytes, export_or_selector) = match target {
            ExecutionKind::WasmBytes(wasm_bytes) => {
                // self.execute_wasm(tracking_copy, address, gas_limit, wasm_bytes, input)
                (wasm_bytes, Either::Left(DEFAULT_WASM_ENTRY_POINT))
            }
            ExecutionKind::Contract { address, selector } => {
                let key = Key::AddressableEntity(EntityAddr::SmartContract(address)); // TODO: Error handling
                let contract = tracking_copy.read(&key).expect("should read contract");

                match contract {
                    Some(StoredValue::ContractV2(ContractV2::V2(manifest))) => {
                        let wasm_key = match manifest.bytecode_addr {
                            ByteCodeAddr::V1CasperWasm(wasm_hash) => {
                                Key::ByteCode(ByteCodeAddr::V1CasperWasm(wasm_hash))
                            }
                            ByteCodeAddr::Empty => {
                                // Note: What should happen?
                                todo!("empty bytecode addr")
                            }
                        };

                        // Note: Bytecode stored in the GlobalStateReader has a "kind" option - currently we know we have a v2 bytecode as the stored contract is of "V2" variant.
                        let wasm_bytes = tracking_copy
                            .read(&wasm_key)
                            .expect("should read wasm")
                            .expect("should have wasm bytes")
                            .into_byte_code()
                            .expect("should be byte code")
                            .take_bytes();

                        let function_index = {
                            // let entry_points = manifest.entry_points;
                            let entry_point = manifest
                                .entry_points
                                .into_iter()
                                .find_map(|entry_point| {
                                    if entry_point.selector == selector.get() {
                                        Some(entry_point)
                                    } else {
                                        None
                                    }
                                })
                                .expect("should find entry point");
                            entry_point.function_index
                        };

                        (Bytes::from(wasm_bytes), Either::Right(function_index))
                    }
                    Some(_stored_contract) => {
                        panic!("Stored contract is not of V2 variant");
                    }
                    None => {
                        panic!("No code found");
                    }
                }
            }
        };

        let mut vm = WasmEngine::new();

        let mut initial_tracking_copy = tracking_copy.fork2();

        let context = Context {
            address,
            storage: tracking_copy,
            executor: ExecutorV2::new(self.config),
        };

        let wasm_instance_config = ConfigBuilder::new()
            .with_gas_limit(gas_limit)
            .with_memory_limit(self.config.memory_limit)
            .with_input(input)
            .build();

        let mut instance = vm.prepare(wasm_bytes, context, wasm_instance_config)?;

        let (vm_result, gas_usage) = match export_or_selector {
            Either::Left(export) => instance.call_export(export),
            Either::Right(function_index) => instance.call_function(function_index),
        };

        let context = instance.teardown();

        let Context {
            storage: final_tracking_copy,
            ..
        } = context;

        match vm_result {
            Ok(()) => Ok(ExecuteResult {
                host_error: None,
                output: None,
                gas_usage,
                tracking_copy_parts: final_tracking_copy.into_raw_parts(),
            }),
            Err(VMError::Return { flags, data }) => {
                let host_error = if flags.contains(ReturnFlags::REVERT) {
                    // The contract has reverted.
                    Some(HostError::CalleeReverted)
                } else {
                    // Merge the tracking copy parts since the execution has succeeded.
                    initial_tracking_copy.merge(final_tracking_copy);

                    None
                };

                Ok(ExecuteResult {
                    host_error,
                    output: data,
                    gas_usage,
                    tracking_copy_parts: initial_tracking_copy.into_raw_parts(),
                })
            }
            Err(VMError::OutOfGas) => Ok(ExecuteResult {
                host_error: Some(HostError::CalleeGasDepleted),
                output: None,
                gas_usage,
                tracking_copy_parts: final_tracking_copy.into_raw_parts(),
            }),
            Err(VMError::Trap(trap_code)) => Ok(ExecuteResult {
                host_error: Some(HostError::CalleeTrapped(trap_code)),
                output: None,
                gas_usage,
                tracking_copy_parts: initial_tracking_copy.into_raw_parts(),
            }),
        }
    }
}
