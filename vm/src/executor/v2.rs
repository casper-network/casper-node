use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use casper_storage::{address_generator, AddressGenerator};
use casper_types::{
    account::AccountHash, contracts::ContractV2, ByteCodeAddr, EntityAddr, HoldsEpoch, Key,
    StoredValue,
};
use either::Either;
use parking_lot::RwLock;
use vm_common::flags::ReturnFlags;

use super::{ExecuteError, ExecuteRequest, ExecuteResult, ExecutionKind, Executor};
use crate::{
    storage::{
        runtime::{self, MintTransferArgs},
        Address, GlobalStateReader, TrackingCopy,
    },
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

impl ExecutorConfigBuilder {
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
    execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
}

impl ExecutorV2 {
    /// Create a new `ExecutorV2` instance.
    pub fn new(config: ExecutorConfig) -> Self {
        let vm = WasmEngine::new();
        ExecutorV2 {
            config,
            vm: Arc::new(vm),
            execution_stack: Default::default(),
        }
    }

    /// Get the context address of the currently executing contract.
    pub(crate) fn context_address(&self) -> Option<Address> {
        match self.execution_stack.read().back() {
            Some(ExecutionKind::Contract { address, .. }) => Some(*address),
            _ => None,
        }
    }

    /// Push the execution stack.
    pub(crate) fn push_execution_stack(&self, execution_kind: ExecutionKind) {
        let mut execution_stack = self.execution_stack.write();
        execution_stack.push_back(execution_kind);
    }

    /// Pop the execution stack.
    pub(crate) fn pop_execution_stack(&self) -> Option<ExecutionKind> {
        let mut execution_stack = self.execution_stack.write();
        execution_stack.pop_back()
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
            gas_limit,
            execution_kind,
            input,
            caller,
            value,
            transaction_hash,
            address_generator,
        } = execute_request;

        // TODO: Purse uref does not need to be optional once value transfers to WasmBytes are supported.
        // let caller_entity_addr = EntityAddr::new_account(caller);
        let stored_value = tracking_copy
            .read(&Key::Account(AccountHash::new(caller)))
            .expect("should read account")
            .expect("should have account");
        let cl_value = stored_value
            .into_cl_value()
            .expect("should be addressable entity");
        let key = cl_value.into_t::<Key>().expect("should be key");
        let stored_value = tracking_copy
            .read(&key)
            .expect("should read account")
            .expect("should have account");
        let addressable_entity = stored_value
            .into_addressable_entity()
            .expect("should be addressable entity");

        let (wasm_bytes, export_or_selector) = match &execution_kind {
            ExecutionKind::WasmBytes(wasm_bytes) => {
                // self.execute_wasm(tracking_copy, address, gas_limit, wasm_bytes, input)
                (wasm_bytes.clone(), Either::Left(DEFAULT_WASM_ENTRY_POINT))
            }
            ExecutionKind::Contract { address, selector } => {
                let key = Key::AddressableEntity(EntityAddr::SmartContract(*address)); // TODO: Error handling
                let contract = tracking_copy.read(&key).expect("should read contract");

                match contract {
                    Some(StoredValue::ContractV2(ContractV2::V2(manifest))) => {
                        // source_uref = Some(manifest.purse_uref);

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

                        if value != 0 {
                            // let purse_uref = source_uref.expect("should have purse uref");

                            let args = {
                                let maybe_to = None;
                                let source = addressable_entity.main_purse();
                                let target = manifest.purse_uref;
                                let amount = value;
                                let id = None;
                                let holds_epoch = HoldsEpoch::NOT_APPLICABLE;
                                MintTransferArgs {
                                    maybe_to,
                                    source,
                                    target,
                                    amount: amount.into(),
                                    id,
                                    holds_epoch,
                                }
                            };

                            runtime::mint_transfer(
                                &mut tracking_copy,
                                transaction_hash,
                                Arc::clone(&address_generator),
                                args,
                            )
                            .expect("Mint transfer to succeed");
                        }

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

        let vm = Arc::clone(&self.vm);

        let mut initial_tracking_copy = tracking_copy.fork2();

        let state_address = match &execution_kind {
            ExecutionKind::Contract { address, .. } => *address,
            ExecutionKind::WasmBytes(_wasm_bytes) => caller,
        };

        let context = Context {
            caller,
            value,
            state_address,
            storage: tracking_copy,
            executor: self.clone(),
            address_generator: Arc::clone(&address_generator),
            transaction_hash,
        };

        let wasm_instance_config = ConfigBuilder::new()
            .with_gas_limit(gas_limit)
            .with_memory_limit(self.config.memory_limit)
            .with_input(input)
            .build();

        let mut instance = vm.prepare(wasm_bytes, context, wasm_instance_config)?;

        self.push_execution_stack(execution_kind.clone());
        let (vm_result, gas_usage) = match export_or_selector {
            Either::Left(export) => instance.call_export(export),
            Either::Right(function_index) => instance.call_function(function_index),
        };
        let top_execution_kind = self
            .pop_execution_stack()
            .expect("should have execution kind"); // SAFETY: We just pushed
        debug_assert_eq!(&top_execution_kind, &execution_kind);

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
