use std::sync::Arc;

use bytes::Bytes;
use casper_types::{contracts::ContractV2, ByteCodeAddr, EntityAddr, Key, StoredValue};
use either::Either;
use vm_common::flags::ReturnFlags;

use super::{ExecuteError, ExecuteRequest, ExecuteResult, ExecutionKind, Executor};
use crate::{
    storage::{GlobalStateReader, TrackingCopy},
    wasm_backend::{Context, WasmInstance},
    ConfigBuilder, HostError, VMError, WasmEngine,
};

const DEFAULT_WASM_ENTRY_POINT: &str = "call";

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
            caller,
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
