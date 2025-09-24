pub mod install;

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
use casper_executor_wasm_host::{
    context::Context,
    system::{self, native_exec, DispatchError, TransferArgs},
};
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteResult,
        ExecuteWithProviderError, ExecuteWithProviderResult, ExecutionKind, Executor, SystemMenu,
    },
    sandboxed_execution::{
        SandboxedExecutionError, SandboxedExecutionRequest, SandboxedExecutionResult,
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
    tracking_copy::TrackingCopyEntityExt,
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, EntityEntryPoint},
    bytesrepr, AddressableEntity, AuctionCosts, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind,
    CLType, ContractRuntimeTag, Digest, EntityAddr, EntityKind, EntryPointAccess, EntryPointAddr,
    EntryPointPayment, EntryPointType, EntryPointValue, Gas, Groups, InitiatorAddr, Key,
    MessageLimits, MintCosts, Package, PackageHash, PackageStatus, Parameters, Phase,
    ProtocolVersion, StorageCosts, StoredValue, TransactionHash, TransactionInvocationTarget, URef,
    WasmV2Config,
};
use install::{InstallContractError, InstallContractRequest, InstallContractResult};
use parking_lot::RwLock;
use tracing::{error, info, warn};

#[cfg(any(feature = "testing", test))]
pub mod chainspec_config;
#[cfg(any(feature = "testing", test))]
pub mod testing;

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
    mint_costs: MintCosts,
    auction_costs: AuctionCosts,
    baseline_motes_amount: u64,
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
    mint_costs: Option<MintCosts>,
    auction_costs: Option<AuctionCosts>,
    baseline_motes_amount: Option<u64>,
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

    /// Set storage costs.
    pub fn with_storage_costs(mut self, storage_costs: StorageCosts) -> Self {
        self.storage_costs = Some(storage_costs);
        self
    }

    /// Set mint costs.
    pub fn with_mint_costs(mut self, mint_costs: MintCosts) -> Self {
        self.mint_costs = Some(mint_costs);
        self
    }

    /// Set auction costs.
    pub fn with_auction_costs(mut self, auction_costs: AuctionCosts) -> Self {
        self.auction_costs = Some(auction_costs);
        self
    }

    pub fn with_baseline_motes_amount(mut self, baseline_motes_amount: u64) -> Self {
        self.baseline_motes_amount = Some(baseline_motes_amount);
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
        let mint_costs = self.mint_costs.ok_or("Storage costs are not set")?;
        let auction_costs = self.auction_costs.ok_or("Storage costs are not set")?;
        let baseline_motes_amount = self
            .baseline_motes_amount
            .ok_or("Baseline motes amount not set")?;
        let message_limits = self.message_limits.ok_or("Message limits are not set")?;

        Ok(ExecutorConfig {
            memory_limit,
            executor_kind,
            wasm_config,
            storage_costs,
            mint_costs,
            auction_costs,
            baseline_motes_amount,
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
            runtime_native_config,
        } = install_request;

        let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&wasm_bytes);

        let caller_key = Key::Account(initiator);
        // TODO: Michal: why is this result not evaluated?
        let _result = get_purse_for_entity(&mut tracking_copy, caller_key);

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

        // 3.1 Store entry points first
        {
            let config = ConfigBuilder::new()
                .with_gas_limit(gas_limit)
                .with_memory_limit(self.config.memory_limit)
                .build()
                .map_err(|config_builder_error| {
                    InstallContractError::Execute(ExecuteError::InternalHost(
                        InternalHostError::ConfigBuilderError(config_builder_error.to_string()),
                    ))
                })?;

            let entry_point_names = casper_executor_wasmer_backend::entry_point_names(
                wasm_bytes, config,
            )
            .map_err(|wasm_prep_error| {
                InstallContractError::Execute(ExecuteError::WasmPreparation(wasm_prep_error))
            })?;

            for name in entry_point_names {
                let entry_point = EntityEntryPoint::new(
                    name.clone(),
                    Parameters::new(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Called,
                    EntryPointPayment::Caller,
                );

                let entry_point_addr = EntryPointAddr::new_v1_entry_point_addr(
                    EntityAddr::SmartContract(smart_contract_addr),
                    &name,
                )
                .map_err(|err| {
                    InstallContractError::GlobalState(GlobalStateError::BytesRepr(err))
                })?;

                let entry_point_key = Key::EntryPoint(entry_point_addr);

                tracking_copy.write(
                    entry_point_key,
                    StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)),
                )
            }
        }

        // TODO: abort(str) as an alternative to trap
        let main_purse: URef = match system::create_purse(
            &mut tracking_copy,
            runtime_native_config.clone(),
            transaction_hash,
            Arc::clone(&address_generator),
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
                    .with_execution_kind(ExecutionKind::Stored {
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
                    .with_runtime_native_config(runtime_native_config)
                    .with_authorization_keys(BTreeSet::from_iter([initiator]))
                    .build()
                    .map_err(InstallContractError::FailedBuildingExecuteRequest)?;

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

    fn execute_system_contract<R: GlobalStateReader + 'static>(
        &self,
        menu_selection: SystemMenu,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        let ExecuteRequest {
            initiator,
            caller_key,
            gas_limit,
            input,
            transaction_hash,
            address_generator,
            sandboxed,
            runtime_native_config,
            ..
        } = execute_request;

        if sandboxed {
            info!("attempt to call system contract while sandboxed");
            return Err(ExecuteError::SandboxedSystemContractCall);
        }

        let gas_usage = GasUsage::new(gas_limit, gas_limit);

        native_exec::<TransferArgs, (), R>(
            tracking_copy,
            runtime_native_config,
            transaction_hash,
            address_generator,
            gas_usage,
            initiator,
            caller_key,
            input,
            menu_selection,
        )
    }

    fn execute_with_tracking_copy<R: GlobalStateReader + 'static>(
        &self,
        mut tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        if let Some(system_menu_selection) = execute_request.execution_kind.system_menu_selection()
        {
            return self.execute_system_contract(
                system_menu_selection,
                tracking_copy,
                execute_request,
            );
        }

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
            sandboxed,
            runtime_native_config,
            authorization_keys,
        } = execute_request;

        let (entity_addr, source_purse) = get_purse_for_entity(&mut tracking_copy, caller_key)?;

        let (wasm_bytes, export_name) = {
            if let ExecutionKind::SessionBytes(wasm_bytes) = &execution_kind {
                (wasm_bytes.clone(), DEFAULT_WASM_ENTRY_POINT)
            } else if let ExecutionKind::Stored {
                address: smart_contract_addr,
                entry_point,
            } = &execution_kind
            {
                let smart_contract_key = Key::SmartContract(*smart_contract_addr);
                let vm1_key = Key::Hash(*smart_contract_addr);

                let mut contract = tracking_copy
                    .read_first(&[&vm1_key, &smart_contract_key])
                    .map_err(|read_error| {
                        error!(
                            "error reading contract under path: {:?}. Details: {read_error}",
                            [&vm1_key, &smart_contract_key]
                        );
                        ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                    })?;

                if let Some(StoredValue::SmartContract(smart_contract_package)) = &contract {
                    let enabled_versions = smart_contract_package.enabled_versions();
                    let maybe_contract_hash = enabled_versions.latest();
                    let contract_hash = if let Some(contract_hash) = maybe_contract_hash {
                        contract_hash
                    } else {
                        //#TODO this probably should not be a node stopping error?
                        error!(
                            "Couldn't find an active version for smart contract under path {:?}",
                            [&vm1_key, &smart_contract_key]
                        );
                        return Err(ExecuteError::NoActiveContract(smart_contract_key));
                    };
                    let entity_addr = EntityAddr::SmartContract(contract_hash.value());
                    let latest_version_key = Key::AddressableEntity(entity_addr);
                    assert_eq!(&entity_addr.value(), smart_contract_addr);
                    let new_contract =
                        tracking_copy
                            .read(&latest_version_key)
                            .map_err(|read_err| {
                                error!("Error when fetching smart contract {latest_version_key}. Details {read_err}");
                                ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                            })?;
                    contract = new_contract;
                };

                match contract {
                    Some(StoredValue::AddressableEntity(addressable_entity)) => {
                        let wasm_key = match addressable_entity.kind() {
                            EntityKind::System(_) => todo!(),
                            EntityKind::Account(_) => todo!(),
                            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1) => {
                                // We need to short circuit here to execute v1 contracts with
                                // vm1 execute
                                let block_info = BlockInfo::new(
                                    state_hash,
                                    block_time,
                                    parent_block_hash,
                                    block_height,
                                    self.execution_engine_v1.config().protocol_version(),
                                );

                                let entity_addr = EntityAddr::SmartContract(*smart_contract_addr);

                                return self.execute_vm1_wasm_byte_code(
                                    initiator,
                                    &entity_addr,
                                    entry_point.clone(),
                                    &input,
                                    &mut tracking_copy,
                                    block_info,
                                    transaction_hash,
                                    gas_limit,
                                    authorization_keys.clone(),
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
                            .map_err(|read_err| {
                                error!(
                                    "Error when fetching wasm_bytes {wasm_key}. Details {read_err}"
                                );
                                ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                            })?
                            .ok_or(ExecuteError::EntityNotFound(wasm_key))?
                            .into_byte_code()
                            .ok_or({
                                error!("Couldn't wasm stored value into ByteCode");
                                ExecuteError::InternalHost(InternalHostError::TypeConversion)
                            })?
                            .take_bytes();

                        if transferred_value != 0 {
                            // TODO: consult w/ Michal re: charge timing
                            let gas_usage = GasUsage::new(gas_limit, gas_limit);

                            let runtime_footprint =
                                match tracking_copy.runtime_footprint_by_entity_addr(entity_addr) {
                                    Ok(footprint) => footprint,
                                    Err(_) => {
                                        return Err(ExecuteError::EntityNotFound(caller_key));
                                    }
                                };
                            match system::transfer(
                                &mut tracking_copy,
                                runtime_footprint,
                                TransferArgs::new(
                                    runtime_native_config.clone(),
                                    transaction_hash,
                                    Arc::clone(&address_generator),
                                    initiator,
                                    caller_key,
                                    gas_usage.remaining_points().into(),
                                    source_purse,
                                    addressable_entity.main_purse(),
                                    transferred_value.into(),
                                ),
                            ) {
                                Ok(()) => {}
                                Err(DispatchError::Internal(internal_error)) => {
                                    error!(
                                        ?internal_error,
                                        "Internal error while transferring value to the contract's purse",
                                    );
                                    return Err(ExecuteError::InternalHost(internal_error));
                                }
                                Err(DispatchError::Call(error)) => {
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
                                Err(error) => {
                                    error!(
                                        ?error,
                                        "Dispatch error while transferring value to the contract's purse",
                                    );
                                    return Err(ExecuteError::InternalHost(
                                        InternalHostError::DispatchSystemContract,
                                    ));
                                }
                            }
                        }

                        (Bytes::from(wasm_bytes), entry_point.as_str())
                    }
                    Some(StoredValue::Contract(_vm1_contract)) => {
                        let block_info = BlockInfo::new(
                            state_hash,
                            block_time,
                            parent_block_hash,
                            block_height,
                            self.execution_engine_v1.config().protocol_version(),
                        );

                        let entity_addr = EntityAddr::SmartContract(*smart_contract_addr);

                        return self.execute_vm1_wasm_byte_code(
                            initiator,
                            &entity_addr,
                            entry_point.clone(),
                            &input,
                            &mut tracking_copy,
                            block_info,
                            transaction_hash,
                            gas_limit,
                            authorization_keys,
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
            } else {
                error!("System executions do not have wasm. This should be unreachable.");
                return Err(ExecuteError::InternalHost(
                    InternalHostError::DispatchSystemContract,
                ));
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
            ExecutionKind::System(_) => {
                error!("System executions are not called in this way. This should be unreachable.");
                return Err(ExecuteError::InternalHost(
                    InternalHostError::DispatchSystemContract,
                ));
            }
        };

        let context = Context {
            initiator,
            config: self.config.wasm_config,
            storage_costs: self.config.storage_costs,
            mint_costs: self.config.mint_costs,
            auction_costs: self.config.auction_costs,
            baseline_motes_amount: self.config.baseline_motes_amount,
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
            sandboxed,
            runtime_native_config,
            parent_block_hash: parent_block_hash.inner().value(),
            block_height,
            authorization_keys,
        };

        // Check that the input argument size does not exceed the VM memory limit
        let memory_limit_bytes = self.config.memory_limit as usize * 65_536; // 64KiB per page
        if context.input.len() > memory_limit_bytes {
            return Err(ExecuteError::ArgumentSizeExceedsMemory {
                argument_size: context.input.len(),
                memory_limit: self.config.memory_limit,
            });
        }

        let wasm_instance_config = ConfigBuilder::new()
            .with_gas_limit(gas_limit)
            .with_memory_limit(self.config.memory_limit)
            .build()
            .map_err(|config_builder_error| {
                ExecuteError::InternalHost(InternalHostError::ConfigBuilderError(
                    config_builder_error.to_string(),
                ))
            })?;

        let mut instance = vm
            .instantiate(wasm_bytes, context, wasm_instance_config)
            .map_err(ExecuteError::WasmPreparation)?;

        self.push_execution_stack(execution_kind.clone());
        let (vm_result, gas_usage) = instance.call_export(export_name);

        let top_execution_kind = self.pop_execution_stack().ok_or({
            //This shouldn't happen since we just pushed
            ExecuteError::InternalHost(InternalHostError::CorruptExecutionState(
                "Unexpected empty execution stack".to_owned(),
            ))
        })?;
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
    fn execute_vm1_wasm_byte_code<R>(
        &self,
        initiator: AccountHash,
        entity_addr: &EntityAddr,
        entry_point: String,
        input: &Bytes,
        tracking_copy: &mut TrackingCopy<R>,
        block_info: BlockInfo,
        transaction_hash: TransactionHash,
        gas_limit: u64,
        authorization_keys: BTreeSet<AccountHash>,
    ) -> Result<ExecuteResult, ExecuteError>
    where
        R: GlobalStateReader + 'static,
    {
        let authorization_keys = if authorization_keys.is_empty() {
            BTreeSet::from_iter([initiator])
        } else {
            authorization_keys
        };
        let initiator_addr = InitiatorAddr::AccountHash(initiator);
        let executable_item =
            ExecutableItem::Invocation(TransactionInvocationTarget::ByHash(entity_addr.value()));
        let entry_point = entry_point.clone();
        let args = bytesrepr::deserialize_from_slice(input)
            .map_err(|err| ExecuteError::InternalHost(InternalHostError::Bytesrepr(err)))?;
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

        let gas_value = wasm_v1_result.consumed().value();
        let gas_consumed = gas_value.try_into().map_err(|msg| {
            error!(
                "Couldn't convert gas ({gas_value}) to u64. Details: {}",
                msg
            );
            ExecuteError::InternalHost(InternalHostError::TypeConversion)
        })?;

        let mut output = wasm_v1_result
            .ret()
            .map(bytesrepr::serialize)
            .map(|maybe| {
                maybe.map_err(|e| ExecuteError::InternalHost(InternalHostError::Bytesrepr(e)))
            })
            .transpose()?
            .map(Bytes::from);

        let host_error = match wasm_v1_result.error() {
            Some(EngineError::Exec(ExecError::GasLimit)) => Some(CallError::CalleeGasDepleted),
            Some(EngineError::Exec(ExecError::Revert(revert_code))) => {
                if output.is_some() {
                    error!("output is not none after ExecutionEngineV1 execution");
                    // ExecutionEngineV1 sets output to None when error occurred.
                    return Err(ExecuteError::InternalHost(
                        InternalHostError::UnexpectedOutput,
                    ));
                }
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

        let remaining_points_res = gas_limit.checked_sub(gas_consumed);

        let remaining_points = if let Some(remaining_points) = remaining_points_res {
            remaining_points
        } else {
            error!("Unable to subtract gas_consumed ({gas_consumed}) from gas_limit ({gas_limit})");
            return Err(ExecuteError::InternalHost(
                InternalHostError::RemainingGasExceedsGasLimit,
            ));
        };

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

    fn execute_sandbox<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        runtime_native_config: RuntimeNativeConfig,
        request: SandboxedExecutionRequest,
    ) -> Result<SandboxedExecutionResult, ExecuteError> {
        // Convert SandboxedExecutionRequest to ExecuteRequest with sandboxed mode enabled
        let execute_request = ExecuteRequestBuilder::default()
            .with_initiator(request.initiator)
            .with_caller_key(Key::Account(request.initiator))
            .with_gas_limit(request.gas_limit) // Use the provided gas limit for protection
            .with_execution_kind(ExecutionKind::Stored {
                address: request.contract_address,
                entry_point: request.entry_point,
            })
            .with_input(Bytes::copy_from_slice(request.input.inner_bytes()))
            .with_transferred_value(0) // Must be 0 for sandboxed queries
            .with_transaction_hash(TransactionHash::from_raw([0; 32])) // Dummy hash for queries
            .with_address_generator(AddressGenerator::new(&[0; 32], Phase::Session))
            .with_chain_name(request.chain_name)
            .with_block_time(request.block_time)
            .with_state_hash(request.state_hash)
            .with_parent_block_hash(request.parent_block_hash)
            .with_block_height(request.block_height)
            .with_sandboxed(true) // Enable sandboxed mode
            .with_runtime_native_config(runtime_native_config)
            .with_authorization_keys(BTreeSet::from_iter([request.initiator]))
            .build()
            .map_err(|error| {
                ExecuteError::InternalHost(InternalHostError::ExecuteRequestBuildFailure(error))
            })?;

        // Execute the query in sandboxed mode
        let execute_result = self.execute_with_tracking_copy(tracking_copy, execute_request)?;
        let output_bytes: Option<Vec<u8>> = execute_result.output.map(|x| x.into());

        // Convert ExecuteResult to SandboxedExecutionResult
        let result = SandboxedExecutionResult {
            error: execute_result
                .host_error
                .map(|call_error| match call_error {
                    CallError::CalleeReverted => SandboxedExecutionError::CalleeReverted,
                    CallError::CalleeTrapped(_) => SandboxedExecutionError::CalleeTrapped,
                    CallError::CalleeGasDepleted => SandboxedExecutionError::CalleeGasDepleted,
                    CallError::NotCallable => SandboxedExecutionError::NotCallable,
                    CallError::Api(api_error) => SandboxedExecutionError::Api(api_error),
                }),
            output: output_bytes.map(|x| x.into()),
            gas_usage: Gas::new(execute_result.gas_usage.gas_spent()),
        };

        Ok(result)
    }
}

fn get_purse_for_entity<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    entity_key: Key,
) -> Result<(EntityAddr, URef), ExecuteError> {
    let stored_value = tracking_copy
        .read(&entity_key)
        .map_err(|_error| ExecuteError::InternalHost(InternalHostError::TrackingCopy))?
        .ok_or(ExecuteError::EntityNotFound(entity_key))?;
    match stored_value {
        StoredValue::CLValue(addressable_entity_key) => {
            let key = addressable_entity_key.into_t::<Key>().map_err(|cl_error| {
                error!("Couldn't convert addressable_entity_key to Key. Details: {cl_error}");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?;
            let hash = match key.into_entity_hash() {
                Some(hash) => hash,
                None => return Err(ExecuteError::EntityNotFound(key)),
            };
            let stored_value = tracking_copy
                .read(&key)
                .map_err(|read_err| {
                    error!("Error when fetching account. Details: {read_err}");
                    ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                })?
                .ok_or({
                    error!("Expected account for {key} to exist");
                    ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                })?;

            let addressable_entity = stored_value.into_addressable_entity().ok_or({
                error!("Error when converting StoredValue to AddressableEntity");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?;
            let addr = addressable_entity.entity_addr(hash);
            Ok((addr, addressable_entity.main_purse()))
        }
        StoredValue::Account(account) => {
            let addr = EntityAddr::Account(account.account_hash().value());
            Ok((addr, account.main_purse()))
        }
        StoredValue::SmartContract(smart_contract_package) => {
            let enabled_versions = smart_contract_package.enabled_versions();
            let maybe_contract_hash = enabled_versions.latest();
            let contract_hash = if let Some(contract_hash) = maybe_contract_hash {
                contract_hash
            } else {
                //#TODO this probably should not be a node stopping error?
                error!("Couldn't find an active version for smart contract {entity_key}");
                return Err(ExecuteError::NoActiveContract(entity_key));
            };

            let entity_addr = EntityAddr::SmartContract(contract_hash.value());
            let latest_version_key = Key::AddressableEntity(entity_addr);
            let new_contract = tracking_copy
                .read(&latest_version_key)
                .map_err(|read_err| {
                    error!("Error when fetching smart contract {latest_version_key}. Details {read_err}");
                    ExecuteError::InternalHost(InternalHostError::TrackingCopy)
                })?;
            let addressable_entity = new_contract
                .ok_or(ExecuteError::EntityNotFound(latest_version_key))?
                .into_addressable_entity()
                .ok_or(ExecuteError::InternalHost(
                    InternalHostError::TypeConversion,
                ))?;

            Ok((entity_addr, addressable_entity.main_purse()))
        }
        other => Err(ExecuteError::InternalHost(
            InternalHostError::UnexpectedStoredValueVariant {
                expected: "AddressableEntity or Account".to_string(),
                found: other.type_name(),
            },
        )),
    }
}
