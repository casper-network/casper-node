pub mod install;
pub(crate) mod system;

use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use bytes::Bytes;
use casper_execution_engine::{
    engine_state::{BlockInfo, Error as EngineError, ExecutableItem, ExecutionEngineV1},
    execution::ExecError,
};
use casper_executor_wasm_common::{
    chain_utils,
    error::{CallError, TrapCode},
    flags::ReturnFlags,
};
use casper_executor_wasm_host::context::Context;
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteResult,
        ExecuteWithProviderError, ExecuteWithProviderResult, ExecutionKind, Executor,
    },
    ConfigBuilder, GasUsage, InternalHostError, VMError, WasmInstance,
};
use casper_executor_wasmer_backend::WasmerEngine;
use casper_storage::{
    global_state::{
        error::Error as GlobalStateError,
        state::{CommitProvider, StateProvider},
        GlobalStateReader,
    },
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys},
    bytesrepr,
    execution::{ExecutorQueryResult, QueryError, VmReadRequest},
    AddressableEntity, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, ContractRuntimeTag,
    Digest, EntityAddr, EntityKind, Gas, Groups, InitiatorAddr, Key, MessageLimits, Package,
    PackageHash, PackageStatus, Phase, ProtocolVersion, StorageCosts, StoredValue, TransactionHash,
    TransactionInvocationTarget, URef, WasmV2Config, U512,
};
use install::{InstallContractError, InstallContractRequest, InstallContractResult};
use parking_lot::RwLock;
use system::{MintArgs, MintTransferArgs};
use tracing::{error, warn};

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
    wasm_config: WasmV2Config,
    storage_costs: StorageCosts,
    message_limits: MessageLimits,
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
    wasm_config: Option<WasmV2Config>,
    storage_costs: Option<StorageCosts>,
    message_limits: Option<MessageLimits>,
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

    /// Set the wasm config.
    pub fn with_wasm_config(mut self, wasm_config: WasmV2Config) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Set the wasm config.
    pub fn with_storage_costs(mut self, storage_costs: StorageCosts) -> Self {
        self.storage_costs = Some(storage_costs);
        self
    }

    /// Set the message limits.
    pub fn with_message_limits(mut self, message_limits: MessageLimits) -> Self {
        self.message_limits = Some(message_limits);
        self
    }

    /// Build the `ExecutorConfig`.
    pub fn build(self) -> Result<ExecutorConfig, &'static str> {
        let memory_limit = self.memory_limit.ok_or("Memory limit is not set")?;
        let executor_kind = self.executor_kind.ok_or("Executor kind is not set")?;
        let wasm_config = self.wasm_config.ok_or("Wasm config is not set")?;
        let storage_costs = self.storage_costs.ok_or("Storage costs are not set")?;
        let message_limits = self.message_limits.ok_or("Message limits are not set")?;

        Ok(ExecutorConfig {
            memory_limit,
            executor_kind,
            wasm_config,
            storage_costs,
            message_limits,
        })
    }
}

#[derive(Clone)]
pub struct ExecutorV2 {
    config: ExecutorConfig,
    compiled_wasm_engine: Arc<WasmerEngine>,
    execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
    execution_engine_v1: Arc<ExecutionEngineV1>,
}

impl ExecutorV2 {
    pub fn install_contract<R>(
        &self,
        state_root_hash: Digest,
        state_provider: &R,
        install_request: InstallContractRequest,
    ) -> Result<InstallContractResult, InstallContractError>
    where
        R: StateProvider + CommitProvider,
        <R as StateProvider>::Reader: 'static,
    {
        let mut tracking_copy = match state_provider.checkout(state_root_hash) {
            Ok(Some(tracking_copy)) => {
                TrackingCopy::new(tracking_copy, 1, state_provider.enable_entity())
            }
            Ok(None) => {
                return Err(InstallContractError::GlobalState(
                    GlobalStateError::RootNotFound,
                ))
            }
            Err(error) => return Err(error.into()),
        };

        let InstallContractRequest {
            initiator,
            gas_limit,
            wasm_bytes,
            entry_point,
            input,
            transferred_value,
            address_generator,
            transaction_hash,
            chain_name,
            block_time,
            seed,
            state_hash,
            parent_block_hash,
            block_height,
        } = install_request;

        let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&wasm_bytes);

        let caller_key = Key::Account(initiator);
        let _source_purse = get_purse_for_entity(&mut tracking_copy, caller_key);

        // 1. Store package hash
        let smart_contract_addr: [u8; 32] = chain_utils::compute_predictable_address(
            chain_name.as_bytes(),
            initiator.value(),
            bytecode_hash,
            seed,
        );

        let mut smart_contract = Package::new(
            Default::default(),
            Default::default(),
            Groups::default(),
            PackageStatus::Unlocked,
        );

        let protocol_version = ProtocolVersion::V2_0_0;
        let protocol_version_major = protocol_version.value().major;

        let next_version = smart_contract.next_entity_version_for(protocol_version_major);

        let entity_version_key = smart_contract.insert_entity_version(
            protocol_version_major,
            EntityAddr::SmartContract(smart_contract_addr),
        );
        debug_assert_eq!(entity_version_key.entity_version(), next_version);

        let smart_contract_addr = chain_utils::compute_predictable_address(
            chain_name.as_bytes(),
            initiator.value(),
            bytecode_hash,
            seed,
        );

        tracking_copy.write(
            Key::SmartContract(smart_contract_addr),
            StoredValue::SmartContract(smart_contract),
        );

        // 2. Store wasm

        let bytecode = ByteCode::new(ByteCodeKind::V2CasperWasm, wasm_bytes.clone().into());
        let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash);

        tracking_copy.write(
            Key::ByteCode(bytecode_addr),
            StoredValue::ByteCode(bytecode),
        );

        // 3. Store addressable entity
        let addressable_entity_key =
            Key::AddressableEntity(EntityAddr::SmartContract(smart_contract_addr));

        // TODO: abort(str) as an alternative to trap
        let main_purse: URef = match system::mint_mint(
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
                return Err(InstallContractError::SystemContract(
                    CallError::CalleeTrapped(TrapCode::UnreachableCodeReached),
                ));
            }
        };

        let addressable_entity = AddressableEntity::new(
            PackageHash::new(smart_contract_addr),
            ByteCodeHash::new(bytecode_hash),
            ProtocolVersion::V2_0_0,
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
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
                    .with_target(ExecutionKind::Stored {
                        address: smart_contract_addr,
                        entry_point: entry_point_name,
                    })
                    .with_gas_limit(gas_limit)
                    .with_input(input)
                    .with_transferred_value(transferred_value)
                    .with_transaction_hash(transaction_hash)
                    .with_shared_address_generator(address_generator)
                    .with_chain_name(chain_name)
                    .with_block_time(block_time)
                    .with_state_hash(state_hash)
                    .with_parent_block_hash(parent_block_hash)
                    .with_block_height(block_height)
                    .build()
                    .expect("should build");

                let forked_tc = tracking_copy.fork2();

                match Self::execute_with_tracking_copy(self, forked_tc, execute_request) {
                    Ok(ExecuteResult {
                        host_error,
                        output,
                        gas_usage,
                        effects,
                        cache,
                        messages,
                    }) => {
                        if let Some(host_error) = host_error {
                            return Err(InstallContractError::Constructor { host_error });
                        }

                        tracking_copy.apply_changes(effects, cache, messages);

                        if let Some(output) = output {
                            warn!(?output, "unexpected output from constructor");
                        }

                        gas_usage
                    }
                    Err(execute_error) => {
                        error!(%execute_error, "unable to execute constructor");
                        return Err(InstallContractError::Execute(execute_error));
                    }
                }
            }
            None => {
                // TODO: Calculate storage gas cost etc. and make it the base cost, then add
                // constructor gas cost
                GasUsage::new(gas_limit, gas_limit)
            }
        };

        let effects = tracking_copy.effects();

        match state_provider.commit_effects(state_root_hash, effects.clone()) {
            Ok(post_state_hash) => Ok(InstallContractResult {
                smart_contract_addr,
                gas_usage: ctor_gas_usage,
                effects,
                post_state_hash,
            }),
            Err(error) => Err(InstallContractError::GlobalState(error)),
        }
    }

    fn execute_with_tracking_copy<R: GlobalStateReader + 'static>(
        &self,
        mut tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        let ExecuteRequest {
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
        } = execute_request;

        // TODO: Purse uref does not need to be optional once value transfers to WasmBytes are
        // supported. let caller_entity_addr = EntityAddr::new_account(caller);
        let source_purse = get_purse_for_entity(&mut tracking_copy, caller_key);

        let (wasm_bytes, export_name) = match &execution_kind {
            ExecutionKind::SessionBytes(wasm_bytes) => {
                // self.execute_wasm(tracking_copy, address, gas_limit, wasm_bytes, input)
                (wasm_bytes.clone(), DEFAULT_WASM_ENTRY_POINT)
            }
            ExecutionKind::Stored {
                address: smart_contract_addr,
                entry_point,
            } => {
                let smart_contract_key = Key::SmartContract(*smart_contract_addr);
                let legacy_key = Key::Hash(*smart_contract_addr);

                let mut contract = tracking_copy
                    .read_first(&[&legacy_key, &smart_contract_key])
                    .expect("should read contract");

                if let Some(StoredValue::SmartContract(smart_contract_package)) = &contract {
                    let contract_hash = smart_contract_package
                        .versions()
                        .latest()
                        .expect("should have last entry");
                    let entity_addr = EntityAddr::SmartContract(contract_hash.value());
                    let latest_version_key = Key::AddressableEntity(entity_addr);
                    assert_eq!(&entity_addr.value(), smart_contract_addr);
                    let new_contract = tracking_copy
                        .read(&latest_version_key)
                        .expect("should read latest version");
                    contract = new_contract;
                };

                match contract {
                    Some(StoredValue::AddressableEntity(addressable_entity)) => {
                        let wasm_key = match addressable_entity.kind() {
                            EntityKind::System(_) => todo!(),
                            EntityKind::Account(_) => todo!(),
                            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1) => {
                                // We need to short circuit here to execute v1 contracts with legacy
                                // execut

                                let block_info = BlockInfo::new(
                                    state_hash,
                                    block_time,
                                    parent_block_hash,
                                    block_height,
                                    self.execution_engine_v1.config().protocol_version(),
                                );

                                let entity_addr = EntityAddr::SmartContract(*smart_contract_addr);

                                return self.execute_legacy_wasm_byte_code(
                                    initiator,
                                    &entity_addr,
                                    entry_point.clone(),
                                    &input,
                                    &mut tracking_copy,
                                    block_info,
                                    transaction_hash,
                                    gas_limit,
                                );
                            }
                            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2) => {
                                Key::ByteCode(ByteCodeAddr::V2CasperWasm(
                                    addressable_entity.byte_code_addr(),
                                ))
                            }
                        };

                        // Note: Bytecode stored in the GlobalStateReader has a "kind" option -
                        // currently we know we have a v2 bytecode as the stored contract is of "V2"
                        // variant.
                        let wasm_bytes = tracking_copy
                            .read(&wasm_key)
                            .expect("should read wasm")
                            .expect("should have wasm bytes")
                            .into_byte_code()
                            .expect("should be byte code")
                            .take_bytes();

                        if transferred_value != 0 {
                            let args = {
                                let maybe_to = None;
                                let source = source_purse;
                                let target = addressable_entity.main_purse();
                                let amount = transferred_value;
                                let id = None;
                                MintTransferArgs {
                                    maybe_to,
                                    source,
                                    target,
                                    amount: amount.into(),
                                    id,
                                }
                            };

                            match system::mint_transfer(
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
                                        gas_usage: GasUsage::new(
                                            gas_limit,
                                            gas_limit - DEFAULT_MINT_TRANSFER_GAS_COST,
                                        ),
                                        effects: tracking_copy.effects(),
                                        cache: tracking_copy.cache(),
                                        messages: tracking_copy.messages(),
                                    });
                                }
                            }
                        }

                        (Bytes::from(wasm_bytes), entry_point.as_str())
                    }
                    Some(StoredValue::Contract(_legacy_contract)) => {
                        let block_info = BlockInfo::new(
                            state_hash,
                            block_time,
                            parent_block_hash,
                            block_height,
                            self.execution_engine_v1.config().protocol_version(),
                        );

                        let entity_addr = EntityAddr::SmartContract(*smart_contract_addr);

                        return self.execute_legacy_wasm_byte_code(
                            initiator,
                            &entity_addr,
                            entry_point.clone(),
                            &input,
                            &mut tracking_copy,
                            block_info,
                            transaction_hash,
                            gas_limit,
                        );
                    }
                    Some(stored_value) => {
                        todo!(
                            "Unexpected {stored_value:?} under key {:?}",
                            &execution_kind
                        );
                    }
                    None => {
                        error!(
                            smart_contract_addr = base16::encode_lower(&smart_contract_addr),
                            ?execution_kind,
                            "No contract code found",
                        );
                        return Err(ExecuteError::CodeNotFound(*smart_contract_addr));
                    }
                }
            }
        };

        let vm = Arc::clone(&self.compiled_wasm_engine);

        let mut initial_tracking_copy = tracking_copy.fork2();

        // Derive callee key from the execution target.
        let callee_key = match &execution_kind {
            ExecutionKind::Stored {
                address: smart_contract_addr,
                ..
            } => Key::SmartContract(*smart_contract_addr),
            ExecutionKind::SessionBytes(_wasm_bytes) => Key::Account(initiator),
        };

        let context = Context {
            initiator,
            config: self.config.wasm_config,
            storage_costs: self.config.storage_costs,
            caller: caller_key,
            callee: callee_key,
            transferred_value,
            tracking_copy,
            executor: self.clone(),
            address_generator: Arc::clone(&address_generator),
            transaction_hash,
            chain_name,
            input,
            block_time,
            message_limits: self.config.message_limits,
            read_only,
        };

        let wasm_instance_config = ConfigBuilder::new()
            .with_gas_limit(gas_limit)
            .with_memory_limit(self.config.memory_limit)
            .build();

        let mut instance = vm.instantiate(wasm_bytes, context, wasm_instance_config)?;

        self.push_execution_stack(execution_kind.clone());
        let (vm_result, gas_usage) = instance.call_export(export_name);

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
                effects: final_tracking_copy.effects(),
                cache: final_tracking_copy.cache(),
                messages: final_tracking_copy.messages(),
            }),
            Err(VMError::Return { flags, data }) => {
                let host_error = if flags.contains(ReturnFlags::REVERT) {
                    // The contract has reverted.
                    Some(CallError::CalleeReverted)
                } else {
                    // Merge the tracking copy parts since the execution has succeeded.
                    initial_tracking_copy.apply_changes(
                        final_tracking_copy.effects(),
                        final_tracking_copy.cache(),
                        final_tracking_copy.messages(),
                    );

                    None
                };

                Ok(ExecuteResult {
                    host_error,
                    output: data,
                    gas_usage,
                    effects: initial_tracking_copy.effects(),
                    cache: initial_tracking_copy.cache(),
                    messages: initial_tracking_copy.messages(),
                })
            }
            Err(VMError::OutOfGas) => Ok(ExecuteResult {
                host_error: Some(CallError::CalleeGasDepleted),
                output: None,
                gas_usage,
                effects: final_tracking_copy.effects(),
                cache: final_tracking_copy.cache(),
                messages: final_tracking_copy.messages(),
            }),
            Err(VMError::Trap(trap_code)) => Ok(ExecuteResult {
                host_error: Some(CallError::CalleeTrapped(trap_code)),
                output: None,
                gas_usage,
                effects: initial_tracking_copy.effects(),
                cache: initial_tracking_copy.cache(),
                messages: initial_tracking_copy.messages(),
            }),
            Err(VMError::Export(export_error)) => {
                error!(?export_error, "export error");
                Ok(ExecuteResult {
                    host_error: Some(CallError::NotCallable),
                    output: None,
                    gas_usage,
                    effects: initial_tracking_copy.effects(),
                    cache: initial_tracking_copy.cache(),
                    messages: initial_tracking_copy.messages(),
                })
            }
            Err(VMError::Execute(execute_error)) => {
                let effects = initial_tracking_copy.effects();
                let cache = initial_tracking_copy.cache();
                let messages = initial_tracking_copy.messages();
                error!(
                    ?execute_error,
                    ?gas_usage,
                    ?effects,
                    ?cache,
                    ?messages,
                    "host error"
                );
                Err(execute_error)
            }
            Err(VMError::Internal(internal_error)) => {
                error!(?internal_error, "internal host error");
                Err(ExecuteError::InternalHost(internal_error))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_legacy_wasm_byte_code<R>(
        &self,
        initiator: AccountHash,
        entity_addr: &EntityAddr,
        entry_point: String,
        input: &Bytes,
        tracking_copy: &mut TrackingCopy<R>,
        block_info: BlockInfo,
        transaction_hash: casper_types::TransactionHash,
        gas_limit: u64,
    ) -> Result<ExecuteResult, ExecuteError>
    where
        R: GlobalStateReader + 'static,
    {
        let authorization_keys = BTreeSet::from_iter([initiator]);
        let initiator_addr = InitiatorAddr::AccountHash(initiator);
        let executable_item =
            ExecutableItem::Invocation(TransactionInvocationTarget::ByHash(entity_addr.value()));
        let entry_point = entry_point.clone();
        let args = bytesrepr::deserialize_from_slice(input).expect("should deserialize");
        let phase = Phase::Session;

        let wasm_v1_result = {
            let forked_tc = tracking_copy.fork2();
            self.execution_engine_v1.execute_with_tracking_copy(
                forked_tc,
                block_info,
                transaction_hash,
                Gas::from(gas_limit),
                initiator_addr,
                executable_item,
                entry_point,
                args,
                authorization_keys,
                phase,
            )
        };

        let effects = wasm_v1_result.effects();
        let messages = wasm_v1_result.messages();

        match wasm_v1_result.cache() {
            Some(cache) => {
                tracking_copy.apply_changes(effects.clone(), cache.clone(), messages.clone());
            }
            None => {
                debug_assert!(
                    effects.is_empty(),
                    "effects should be empty if there is no cache"
                );
            }
        }

        let gas_consumed = wasm_v1_result
            .consumed()
            .value()
            .try_into()
            .expect("Should convert consumed gas to u64");

        let mut output = wasm_v1_result
            .ret()
            .map(|ret| bytesrepr::serialize(ret).unwrap())
            .map(Bytes::from);

        let host_error = match wasm_v1_result.error() {
            Some(EngineError::Exec(ExecError::GasLimit)) => Some(CallError::CalleeGasDepleted),
            Some(EngineError::Exec(ExecError::Revert(revert_code))) => {
                assert!(output.is_none(), "output should be None"); // ExecutionEngineV1 sets output to None when error occurred.
                let revert_code: u32 = (*revert_code).into();
                output = Some(revert_code.to_le_bytes().to_vec().into()); // Pass serialized revert code as output.
                Some(CallError::CalleeReverted)
            }
            Some(_) => Some(CallError::CalleeTrapped(TrapCode::UnreachableCodeReached)),
            None => None,
        };

        // TODO: Support multisig

        // TODO: Convert this to a host error as if it was executed.

        // SAFETY: Gas limit is first promoted from u64 to u512, and we know
        // consumed gas under v1 would not exceed the imposed limit therefore an
        // unwrap here is safe.

        let remaining_points = gas_limit.checked_sub(gas_consumed).unwrap();

        let fork2 = tracking_copy.fork2();
        Ok(ExecuteResult {
            host_error,
            output,
            gas_usage: GasUsage::new(gas_limit, remaining_points),
            effects: fork2.effects(),
            cache: fork2.cache(),
            messages: fork2.messages(),
        })
    }

    pub fn execute_with_provider<R>(
        &self,
        state_root_hash: Digest,
        state_provider: &R,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteWithProviderResult, ExecuteWithProviderError>
    where
        R: StateProvider + CommitProvider,
        <R as StateProvider>::Reader: 'static,
    {
        let tracking_copy = match state_provider.checkout(state_root_hash) {
            Ok(Some(tracking_copy)) => tracking_copy,
            Ok(None) => {
                return Err(ExecuteWithProviderError::GlobalState(
                    GlobalStateError::RootNotFound,
                ))
            }
            Err(global_state_error) => return Err(global_state_error.into()),
        };

        let tracking_copy = TrackingCopy::new(tracking_copy, 1, state_provider.enable_entity());

        match self.execute_with_tracking_copy(tracking_copy, execute_request) {
            Ok(ExecuteResult {
                host_error,
                output,
                gas_usage,
                effects,
                cache: _,
                messages,
            }) => match state_provider.commit_effects(state_root_hash, effects.clone()) {
                Ok(post_state_hash) => Ok(ExecuteWithProviderResult::new(
                    host_error,
                    output,
                    gas_usage,
                    effects,
                    post_state_hash,
                    messages,
                )),
                Err(error) => Err(error.into()),
            },
            Err(error) => Err(ExecuteWithProviderError::Execute(error)),
        }
    }
}

impl ExecutorV2 {
    /// Create a new `ExecutorV2` instance.
    pub fn new(config: ExecutorConfig, execution_engine_v1: Arc<ExecutionEngineV1>) -> Self {
        let wasm_engine = match config.executor_kind {
            ExecutorKind::Compiled => WasmerEngine::new(),
        };
        ExecutorV2 {
            config,
            compiled_wasm_engine: Arc::new(wasm_engine),
            execution_stack: Default::default(),
            execution_engine_v1,
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
    /// Returns an error if the execution fails. This can happen if the Wasm instance cannot be
    /// prepared. Otherwise, returns the result of the execution with a gas usage attached which
    /// means a successful execution (that may or may not have produced an error such as a trap,
    /// return, or out of gas).
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        self.execute_with_tracking_copy(tracking_copy, execute_request)
    }

    fn read_query<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        request: VmReadRequest,
    ) -> Result<ExecutorQueryResult, ExecuteError> {
        // Convert VmReadRequest to ExecuteRequest with read-only mode enabled
        let execute_request = ExecuteRequestBuilder::default()
            .with_initiator(request.initiator)
            .with_caller_key(Key::Account(request.initiator))
            .with_gas_limit(request.gas_limit) // Use the provided gas limit for protection
            .with_target(ExecutionKind::Stored {
                address: request.contract_address,
                entry_point: request.entry_point,
            })
            .with_input(request.input.into())
            .with_transferred_value(0) // Must be 0 for read-only queries
            .with_transaction_hash(TransactionHash::from_raw([0; 32])) // Dummy hash for queries
            .with_address_generator(AddressGenerator::new(&[0; 32], Phase::Session))
            .with_chain_name(request.chain_name)
            .with_block_time(request.block_time)
            .with_state_hash(request.state_hash)
            .with_parent_block_hash(request.parent_block_hash)
            .with_block_height(request.block_height)
            .with_read_only(true) // Enable read-only mode
            .build()
            .map_err(|_| {
                ExecuteError::InternalHost(InternalHostError::ExecuteRequestBuildFailure)
            })?;

        // Execute the query in read-only mode
        let execute_result = self.execute_with_tracking_copy(tracking_copy, execute_request)?;
        let output_bytes: Option<Vec<u8>> = execute_result.output.map(|x| x.into());

        // Convert ExecuteResult to VmReadResult
        let query_result = ExecutorQueryResult {
            error: execute_result
                .host_error
                .map(|call_error| match call_error {
                    CallError::CalleeReverted => QueryError::CalleeReverted,
                    CallError::CalleeTrapped(_) => QueryError::CalleeTrapped,
                    CallError::CalleeGasDepleted => QueryError::CalleeGasDepleted,
                    CallError::NotCallable => QueryError::NotCallable,
                }),
            output: output_bytes.map(|x| x.into()),
            gas_usage: Gas::new(execute_result.gas_usage.gas_spent()),
        };

        Ok(query_result)
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
        StoredValue::Account(account) => account.main_purse(),
        StoredValue::SmartContract(smart_contract_package) => {
            let contract_hash = smart_contract_package
                .versions()
                .latest()
                .expect("should have last entry");
            let entity_addr = EntityAddr::SmartContract(contract_hash.value());
            let latest_version_key = Key::AddressableEntity(entity_addr);
            let new_contract = tracking_copy
                .read(&latest_version_key)
                .expect("should read latest version");
            let addressable_entity = new_contract
                .expect("should have addressable entity")
                .into_addressable_entity()
                .expect("should be addressable entity");
            addressable_entity.main_purse()
        }
        other => panic!("should be account or contract received {other:?}"),
    }
}
