use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use casper_storage::{address_generator, tracking_copy::TrackingCopyExt, AddressGenerator};
use casper_types::{
    account::AccountHash,
    addressable_entity::{self, ActionThresholds, AssociatedKeys, MessageTopics},
    bytesrepr::ToBytes,
    contracts::{ContractHash, ContractPackageHash},
    system, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, Digest, EntityAddr, EntityKind, EntryPointAddr, EntryPointValue, Groups,
    HoldsEpoch, Key, Package, PackageHash, PackageStatus, ProtocolVersion, StoredValue,
    TransactionRuntime, URef, U512,
};
use either::Either;
use parking_lot::RwLock;
use rand::Rng;
use tracing::{error, warn};
use vm_common::flags::{EntryPointFlags, ReturnFlags};
use wasmer_types::compilation::function;

use super::{
    CreateContractRequest, ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteResult,
    ExecutionKind, Executor, StoreContractError, StoreContractResult,
};
use crate::{
    storage::{
        runtime::{self, MintArgs, MintTransferArgs},
        Address, GlobalStateReader, TrackingCopy,
    },
    wasm_backend::{Context, GasUsage, WasmInstance},
    ConfigBuilder, HostError, TrapCode, VMError, WasmEngine,
};

const DEFAULT_WASM_ENTRY_POINT: &str = "call";

const DEFAULT_MINT_TRANSFER_GAS_COST: u64 = 1; // NOTE: Require gas while executing and set this to at least 100_000_000 (or use chainspec)

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
    fn create_contract<R: GlobalStateReader + 'static>(
        &self,
        mut tracking_copy: TrackingCopy<R>,
        store_request: CreateContractRequest,
    ) -> Result<StoreContractResult, StoreContractError> {
        let CreateContractRequest {
            initiator,
            gas_limit,
            wasm_bytes,
            entry_point,
            input,
            value,
            address_generator,
            transaction_hash,
        } = store_request;

        let caller_key = Key::Account(AccountHash::new(initiator.into()));
        let source_purse = get_purse_for_entity(&mut tracking_copy, caller_key);

        // 1. Store package hash
        let contract_package = Package::new(
            Default::default(),
            Default::default(),
            Groups::default(),
            PackageStatus::Unlocked,
        );

        let mut rng = rand::thread_rng(); // TODO: Deterministic random number generator
        let package_hash_bytes: Address = rng.gen(); // TODO: Do we need packages at all in new VM?
        let contract_hash: Address = rng.gen();

        tracking_copy.write(
            Key::Package(package_hash_bytes),
            StoredValue::Package(contract_package),
        );

        // 2. Store wasm
        let bytecode_hash = Digest::hash(&wasm_bytes);

        let bytecode = ByteCode::new(ByteCodeKind::V1CasperWasm, wasm_bytes.clone().into());
        let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash.value());

        tracking_copy.write(
            Key::ByteCode(bytecode_addr),
            StoredValue::ByteCode(bytecode),
        );

        // 3. Store addressable entity

        let entity_addr = EntityAddr::SmartContract(contract_hash);
        let addressable_entity_key = Key::AddressableEntity(entity_addr);

        // TODO: abort(str) as an alternative to trap
        let transaction_hash = transaction_hash;
        let main_purse: URef = match runtime::mint_mint(
            &mut tracking_copy,
            transaction_hash,
            Arc::clone(&address_generator),
            MintArgs {
                initial_balance: U512::zero(),
            },
        ) {
            Ok(uref) => uref,
            Err(mint_error) => {
                error!(?mint_error, "Failed to create a purse");
                return Err(StoreContractError::SystemContract(
                    HostError::CalleeTrapped(TrapCode::UnreachableCodeReached),
                ));
            }
        };

        // for entrypoint in entry_points {
        //     let key = Key::EntryPoint(EntryPointAddr::VmCasperV2 {
        //         entity_addr,
        //         selector: entrypoint.selector,
        //     });
        //     let value = EntryPointValue::V2CasperVm(EntryPointV2 {
        //         function_index: entrypoint.fptr,
        //         flags: entrypoint.flags,
        //     });
        //     let stored_value = StoredValue::EntryPoint(value);
        //     caller.context_mut().tracking_copy.write(key, stored_value);
        // }

        let addressable_entity = AddressableEntity::new(
            PackageHash::new(package_hash_bytes),
            ByteCodeHash::new(bytecode_hash.value()),
            ProtocolVersion::V2_0_0,
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::SmartContract(TransactionRuntime::VmCasperV2),
        );

        tracking_copy.write(
            addressable_entity_key,
            StoredValue::AddressableEntity(addressable_entity),
        );

        let ctor_gas_usage = match entry_point {
            Some(entry_point_name) => {
                let input = input.unwrap_or_default();
                let execute_request = ExecuteRequestBuilder::default()
                    .with_initiator(initiator)
                    .with_caller_key(caller_key)
                    .with_callee_key(addressable_entity_key)
                    .with_target(ExecutionKind::Stored {
                        address: entity_addr,
                        entry_point: entry_point_name,
                    })
                    .with_gas_limit(gas_limit)
                    .with_input(input)
                    .with_value(value)
                    .with_transaction_hash(transaction_hash)
                    .with_shared_address_generator(address_generator)
                    .build()
                    .expect("should build");

                let forked_tc = tracking_copy.fork2();

                match Self::execute(&self, forked_tc, execute_request) {
                    Ok(ExecuteResult {
                        host_error,
                        output,
                        gas_usage,
                        tracking_copy_parts,
                    }) => {
                        if let Some(host_error) = host_error {
                            return Err(StoreContractError::Constructor { host_error });
                        }

                        tracking_copy.merge_raw_parts(tracking_copy_parts);

                        if let Some(output) = output {
                            warn!(?output, "unexpected output from constructor");
                        }

                        gas_usage
                    }
                    Err(error) => {
                        error!(%error, "unable to execute constructor");
                        return Err(StoreContractError::Execute(error));
                    }
                }
            }
            None => {
                // TODO: Calculate storage gas cost etc. and make it the base cost, then add constructor gas cost
                GasUsage::new(gas_limit, gas_limit)
            }
        };

        Ok(StoreContractResult {
            contract_package_hash: ContractPackageHash::new(package_hash_bytes),
            contract_hash: ContractHash::new(contract_hash),
            version: 1,
            gas_usage: ctor_gas_usage,
            tracking_copy_parts: tracking_copy.into_raw_parts(),
        })
    }

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
        let source_purse = get_purse_for_entity(&mut tracking_copy, caller_key);

        let (wasm_bytes, export_or_selector): (_, Either<&str, u32>) = match &execution_kind {
            ExecutionKind::SessionBytes(wasm_bytes) => {
                // self.execute_wasm(tracking_copy, address, gas_limit, wasm_bytes, input)
                (wasm_bytes.clone(), Either::Left(DEFAULT_WASM_ENTRY_POINT))
            }
            ExecutionKind::Stored {
                address: entity_addr,
                entry_point,
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

                            match runtime::mint_transfer(
                                &mut tracking_copy,
                                transaction_hash,
                                Arc::clone(&address_generator),
                                args,
                            ) {
                                Ok(()) => {
                                    // Transfer succeed, go on
                                }
                                Err(error) => {
                                    return Ok(ExecuteResult {
                                        host_error: Some(error),
                                        output: None,
                                        gas_usage: GasUsage {
                                            gas_limit,
                                            remaining_points: gas_limit
                                                - DEFAULT_MINT_TRANSFER_GAS_COST,
                                        },
                                        tracking_copy_parts: tracking_copy.into_raw_parts(),
                                    });
                                }
                            }
                        }

                        (Bytes::from(wasm_bytes), Either::Left(entry_point.as_str()))
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
            ExecutionKind::SessionBytes(_wasm_bytes) => Key::Account(AccountHash::new(initiator)),
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
            Either::Left(export_name) => instance.call_export(export_name),
            Either::Right(_entry_point) => todo!("Restore selectors"), //instance.call_export(&entry_point),
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
            Err(VMError::Export(export_error)) => {
                error!(?export_error, "export error");
                Ok(ExecuteResult {
                    host_error: Some(HostError::NotCallable),
                    output: None,
                    gas_usage,
                    tracking_copy_parts: initial_tracking_copy.into_raw_parts(),
                })
            }
        }
    }
}

fn get_purse_for_entity<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    entity_key: Key,
) -> casper_types::URef {
    let stored_value = tracking_copy
        .read(&entity_key)
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
        StoredValue::AddressableEntity(addressable_entity) => addressable_entity.main_purse(),
        other => panic!("should be account or contract received {other:?}"),
    }
}
