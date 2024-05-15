use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use casper_storage::{address_generator, tracking_copy::TrackingCopyExt, AddressGenerator};
use casper_types::{
    account::AccountHash, addressable_entity, system, AddressableEntityHash, ByteCodeAddr,
    EntityAddr, EntityKind, EntryPointAddr, EntryPointValue, HoldsEpoch, Key, PackageHash,
    StoredValue, TransactionRuntime,
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
            initiator,
            caller_key,
            callee_key,
            gas_limit,
            execution_kind,
            input,
            value,
            transaction_hash,
            address_generator,
        } = execute_request;

        // TODO: Purse uref does not need to be optional once value transfers to WasmBytes are supported.
        // let caller_entity_addr = EntityAddr::new_account(caller);
        let source_purse = {
            let stored_value = tracking_copy
                .read(&caller_key)
                .expect("should read account")
                .expect("should have account");
            match stored_value {
                StoredValue::CLValue(addressable_entity_key) => {
                    let key = addressable_entity_key
                        .into_t::<Key>()
                        .expect("should be key");
                    let stored_value = tracking_copy
                        .read(&key)
                        .expect("should read account")
                        .expect("should have account");

                    let addressable_entity = stored_value
                        .into_addressable_entity()
                        .expect("should be addressable entity");

                    addressable_entity.main_purse()
                }
                StoredValue::AddressableEntity(addressable_entity) => {
                    addressable_entity.main_purse()
                }
                other => panic!("should be account or contract received {other:?}"),
            }
        };

        let (wasm_bytes, export_or_selector) = match &execution_kind {
            ExecutionKind::WasmBytes(wasm_bytes) => {
                // self.execute_wasm(tracking_copy, address, gas_limit, wasm_bytes, input)
                (wasm_bytes.clone(), Either::Left(DEFAULT_WASM_ENTRY_POINT))
            }
            ExecutionKind::Stored {
                address: entity_addr,
                selector,
            } => {
                let key = Key::AddressableEntity(*entity_addr); // TODO: Error handling

                let contract = tracking_copy.read(&key).expect("should read contract");

                match contract {
                    Some(StoredValue::AddressableEntity(addressable_entity)) => {
                        let wasm_key = match addressable_entity.kind() {
                            EntityKind::System(_) => todo!(),
                            EntityKind::Account(_) => todo!(),
                            EntityKind::SmartContract(TransactionRuntime::VmCasperV1) => {
                                todo!()
                            }
                            EntityKind::SmartContract(TransactionRuntime::VmCasperV2) => {
                                // OK
                                Key::ByteCode(ByteCodeAddr::V2CasperWasm(
                                    addressable_entity.byte_code_addr(),
                                ))
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
                            // let entry_points = tracking_copy.get_v2_entry_points(*entity_addr);
                            // dbg!(&entry_points);

                            let entry_point_addr = EntryPointAddr::VmCasperV2 {
                                entity_addr: *entity_addr,
                                selector: selector.map(|selector| selector.get()),
                            };
                            // dbg!(&entry_point_addr);
                            let key = Key::EntryPoint(entry_point_addr);
                            // dbg!(&key);
                            match tracking_copy.read(&key) {
                                Ok(Some(StoredValue::EntryPoint(EntryPointValue::V2CasperVm(
                                    entrypoint_v2,
                                )))) => entrypoint_v2.function_index,
                                Ok(Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(
                                    entrypoint_v1,
                                )))) => {
                                    panic!("Unexpected V1 entry point found: {entrypoint_v1:?}");
                                }
                                Ok(Some(stored_value)) => {
                                    panic!("Unexpected entry point found: {stored_value:?}");
                                }
                                Ok(None) => {
                                    panic!("No entry point found for address {key:?} and selector {selector:?}");
                                }
                                Err(error) => panic!("Error reading entry point: {error:?}"),
                            }
                        };

                        if value != 0 {
                            let args = {
                                let maybe_to = None;
                                let source = source_purse;
                                let target = addressable_entity.main_purse();
                                let amount = value;
                                let id = None;
                                MintTransferArgs {
                                    maybe_to,
                                    source,
                                    target,
                                    amount: amount.into(),
                                    id,
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

        let callee_key = match &execution_kind {
            ExecutionKind::Stored {
                address: entity_addr,
                ..
            } => Key::AddressableEntity(*entity_addr),
            ExecutionKind::WasmBytes(_wasm_bytes) => Key::Account(AccountHash::new(initiator)),
        };

        let context = Context {
            initiator: initiator,
            caller: caller_key,
            callee: callee_key,
            value,
            tracking_copy,
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
            tracking_copy: final_tracking_copy,
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
