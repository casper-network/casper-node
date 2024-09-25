mod chain_utils;
pub mod install;
pub(crate) mod system;

use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use bytes::Bytes;
use casper_execution_engine::{
    engine_state::{EngineConfig, Error as EngineError, ExecutableItem, ExecutionEngineV1},
    execution::ExecError,
};
use casper_executor_wasm_common::flags::ReturnFlags;
use casper_executor_wasm_host::context::Context;
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteResult,
        ExecuteWithProviderError, ExecuteWithProviderResult, ExecutionKind, Executor,
    },
    ConfigBuilder, GasUsage, HostError, TrapCode, VMError, WasmInstance,
};
use casper_executor_wasmer_backend::WasmerEngine;
use casper_storage::{
    global_state::{
        error::Error as GlobalStateError,
        state::{CommitProvider, StateProvider},
        GlobalStateReader,
    },
    AddressGeneratorBuilder, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopics},
    bytesrepr,
    contracts::{ContractHash, ContractPackageHash},
    execution::Effects,
    AddressableEntity, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, Digest,
    EntityAddr, EntityKind, Gas, Groups, InitiatorAddr, Key, Package, PackageHash, PackageStatus,
    Phase, ProtocolVersion, StoredValue, Timestamp, Transaction, TransactionEntryPoint,
    TransactionInvocationTarget, TransactionRuntime, TransactionTarget, URef, U512,
};
use either::Either;
use install::{
    InstallContractError, InstallContractRequest, InstallContractRequestBuilder,
    InstallContractResult,
};
use parking_lot::RwLock;
use system::{MintArgs, MintTransferArgs};
use thiserror::Error;
use tracing::{error, info, warn};

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
    compiled_wasm_engine: Arc<WasmerEngine>,
    execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
    execution_engine_v1: ExecutionEngineV1,
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
            Ok(Some(tracking_copy)) => TrackingCopy::new(tracking_copy, 0),
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
        } = install_request;

        let caller_key = Key::Account(initiator);
        let _source_purse = get_purse_for_entity(&mut tracking_copy, caller_key);

        // 1. Store package hash
        let contract_package = Package::new(
            Default::default(),
            Default::default(),
            Groups::default(),
            PackageStatus::Unlocked,
        );

        let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&wasm_bytes);

        let package_hash_bytes: [u8; 32];
        let contract_hash = chain_utils::compute_predictable_address(
            chain_name.as_bytes(),
            initiator,
            bytecode_hash,
        );

        {
            let mut gen = address_generator.write();
            package_hash_bytes = gen.create_address(); // TODO: Do we need packages at all in new
                                                       // VM?
                                                       // contract_hash = gen.create_address();
        }

        tracking_copy.write(
            Key::Package(package_hash_bytes),
            StoredValue::Package(contract_package),
        );

        // 2. Store wasm

        let bytecode = ByteCode::new(ByteCodeKind::V1CasperWasm, wasm_bytes.clone().into());
        let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash);

        tracking_copy.write(
            Key::ByteCode(bytecode_addr),
            StoredValue::ByteCode(bytecode),
        );

        // 3. Store addressable entity

        let entity_addr = EntityAddr::SmartContract(contract_hash);
        let addressable_entity_key = Key::AddressableEntity(entity_addr);

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
                    HostError::CalleeTrapped(TrapCode::UnreachableCodeReached),
                ));
            }
        };

        let addressable_entity = AddressableEntity::new(
            PackageHash::new(package_hash_bytes),
            ByteCodeHash::new(bytecode_hash),
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
                    .with_transferred_value(transferred_value)
                    .with_transaction_hash(transaction_hash)
                    .with_shared_address_generator(address_generator)
                    .with_chain_name(chain_name)
                    .with_block_time(block_time)
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
                    }) => {
                        if let Some(host_error) = host_error {
                            return Err(InstallContractError::Constructor { host_error });
                        }

                        tracking_copy.apply_changes(effects, cache);

                        if let Some(output) = output {
                            warn!(?output, "unexpected output from constructor");
                        }

                        gas_usage
                    }
                    Err(error) => {
                        error!(%error, "unable to execute constructor");
                        return Err(InstallContractError::Execute(error));
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

        match state_provider.commit(state_root_hash, effects.clone()) {
            Ok(post_state_hash) => Ok(InstallContractResult {
                contract_package_hash: ContractPackageHash::new(package_hash_bytes),
                contract_hash: ContractHash::new(contract_hash),
                version: 1,
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
            callee_key: _,
            gas_limit,
            execution_kind,
            input,
            transferred_value,
            transaction_hash,
            address_generator,
            chain_name,
            block_time,
        } = execute_request;

        // TODO: Purse uref does not need to be optional once value transfers to WasmBytes are
        // supported. let caller_entity_addr = EntityAddr::new_account(caller);
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
                let legacy_key = Key::Hash(entity_addr.value());

                let contract = tracking_copy
                    .read_first(&[&key, &legacy_key])
                    .expect("should read contract");

                match contract {
                    Some(StoredValue::AddressableEntity(addressable_entity)) => {
                        let wasm_key = match addressable_entity.kind() {
                            EntityKind::System(_) => todo!(),
                            EntityKind::Account(_) => todo!(),
                            EntityKind::SmartContract(TransactionRuntime::VmCasperV1) => {
                                // We need to short circuit here to execute v1 contracts with legacy
                                // execut

                                return self.execute_legacy_wasm_byte_code(
                                    initiator,
                                    block_time,
                                    entity_addr,
                                    entry_point,
                                    &input,
                                    &mut tracking_copy,
                                    transaction_hash,
                                    gas_limit,
                                );
                            }
                            EntityKind::SmartContract(TransactionRuntime::VmCasperV2) => {
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
                                    });
                                }
                            }
                        }

                        (Bytes::from(wasm_bytes), Either::Left(entry_point.as_str()))
                    }
                    Some(StoredValue::Contract(_legacy_contract)) => {
                        return self.execute_legacy_wasm_byte_code(
                            initiator,
                            block_time,
                            entity_addr,
                            entry_point,
                            &input,
                            &mut tracking_copy,
                            transaction_hash,
                            gas_limit,
                        );
                    }
                    Some(stored_value) => {
                        todo!("Not yet implemented {stored_value:?}");
                    }
                    None => {
                        panic!("No code found");
                    }
                }
            }
        };

        let vm = Arc::clone(&self.compiled_wasm_engine);

        let mut initial_tracking_copy = tracking_copy.fork2();

        let callee_key = match &execution_kind {
            ExecutionKind::Stored {
                address: entity_addr,
                ..
            } => Key::AddressableEntity(*entity_addr),
            ExecutionKind::SessionBytes(_wasm_bytes) => Key::Account(initiator),
        };

        let context = Context {
            initiator,
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
        };

        let wasm_instance_config = ConfigBuilder::new()
            .with_gas_limit(gas_limit)
            .with_memory_limit(self.config.memory_limit)
            .build();

        let mut instance = vm.instantiate(wasm_bytes, context, wasm_instance_config)?;

        self.push_execution_stack(execution_kind.clone());
        let (vm_result, gas_usage) = match export_or_selector {
            Either::Left(export_name) => instance.call_export(export_name),
            Either::Right(_entry_point) => todo!("Restore selectors"), /* instance.call_export(&
                                                                        * entry_point), */
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
                effects: final_tracking_copy.effects(),
                cache: final_tracking_copy.cache(),
            }),
            Err(VMError::Return { flags, data }) => {
                let host_error = if flags.contains(ReturnFlags::REVERT) {
                    // The contract has reverted.
                    Some(HostError::CalleeReverted)
                } else {
                    // Merge the tracking copy parts since the execution has succeeded.
                    initial_tracking_copy
                        .apply_changes(final_tracking_copy.effects(), final_tracking_copy.cache());

                    None
                };

                Ok(ExecuteResult {
                    host_error,
                    output: data,
                    gas_usage,
                    effects: initial_tracking_copy.effects(),
                    cache: initial_tracking_copy.cache(),
                })
            }
            Err(VMError::OutOfGas) => Ok(ExecuteResult {
                host_error: Some(HostError::CalleeGasDepleted),
                output: None,
                gas_usage,
                effects: final_tracking_copy.effects(),
                cache: final_tracking_copy.cache(),
            }),
            Err(VMError::Trap(trap_code)) => Ok(ExecuteResult {
                host_error: Some(HostError::CalleeTrapped(trap_code)),
                output: None,
                gas_usage,
                effects: initial_tracking_copy.effects(),
                cache: initial_tracking_copy.cache(),
            }),
            Err(VMError::Export(export_error)) => {
                error!(?export_error, "export error");
                Ok(ExecuteResult {
                    host_error: Some(HostError::NotCallable),
                    output: None,
                    gas_usage,
                    effects: initial_tracking_copy.effects(),
                    cache: initial_tracking_copy.cache(),
                })
            }
        }
    }

    fn execute_legacy_wasm_byte_code<R>(
        &self,
        initiator: AccountHash,
        block_time: Timestamp,
        entity_addr: &EntityAddr,
        entry_point: &String,
        input: &Bytes,
        tracking_copy: &mut TrackingCopy<R>,
        transaction_hash: casper_types::TransactionHash,
        gas_limit: u64,
    ) -> Result<ExecuteResult, ExecuteError>
    where
        R: GlobalStateReader + 'static,
    {
        let authorization_keys = BTreeSet::from_iter([initiator]);
        let initiator_addr = InitiatorAddr::AccountHash(initiator);
        let block_time = BlockTime::from(block_time);
        let executable_item =
            ExecutableItem::Invocation(TransactionInvocationTarget::ByHash(entity_addr.value()));
        let entry_point = entry_point.clone();
        let args = bytesrepr::deserialize_from_slice(input).expect("should deserialize");
        let phase = Phase::Session;
        let wasm_v1_result = {
            let forked_tc = tracking_copy.fork2();
            self.execution_engine_v1.execute_with_tracking_copy(
                forked_tc,
                block_time,
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
        match wasm_v1_result.cache() {
            Some(cache) => {
                tracking_copy.apply_changes(effects.clone(), cache.clone());
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
            .map(|ret| bytesrepr::serialize(&ret).unwrap())
            .map(Bytes::from);

        let host_error = match wasm_v1_result.error() {
            Some(EngineError::Exec(ExecError::GasLimit)) => Some(HostError::CalleeGasDepleted),
            Some(EngineError::Exec(ExecError::Revert(revert_code))) => {
                assert!(output.is_none(), "output should be None"); // ExecutionEngineV1 sets output to None when error occurred.
                let revert_code: u32 = (*revert_code).into();
                output = Some(revert_code.to_le_bytes().to_vec().into()); // Pass serialized revert code as output.
                Some(HostError::CalleeReverted)
            }
            Some(_) => Some(HostError::CalleeTrapped(TrapCode::UnreachableCodeReached)),
            None => None,
        };

        // TODO: Support multisig

        // TODO: Convert this to a host error as if it was executed.

        // SAFETY: Gas limit is first promoted from u64 to u512, and we know
        // consumed gas under v1 would not exceed the imposed limit therefore an
        // unwrap here is safe.

        let remaining_points = gas_limit.checked_sub(gas_consumed).unwrap();

        let fork2 = tracking_copy.fork2();
        return Ok(ExecuteResult {
            host_error,
            output,
            gas_usage: GasUsage::new(gas_limit, remaining_points),
            effects: fork2.effects(),
            cache: fork2.cache(),
        });
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

        let tracking_copy = TrackingCopy::new(tracking_copy, 1);

        match self.execute_with_tracking_copy(tracking_copy, execute_request) {
            Ok(ExecuteResult {
                host_error,
                output,
                gas_usage,
                effects,
                cache: _,
            }) => match state_provider.commit(state_root_hash, effects.clone()) {
                Ok(post_state_hash) => Ok(ExecuteWithProviderResult {
                    host_error,
                    output,
                    gas_usage,
                    post_state_hash,
                    effects,
                }),
                Err(error) => Err(error.into()),
            },
            Err(error) => Err(ExecuteWithProviderError::Execute(error)),
        }
    }
}

impl ExecutorV2 {
    /// Create a new `ExecutorV2` instance.
    pub fn new(config: ExecutorConfig) -> Self {
        let wasm_engine = match config.executor_kind {
            ExecutorKind::Compiled => WasmerEngine::new(),
        };
        ExecutorV2 {
            config,
            compiled_wasm_engine: Arc::new(wasm_engine),
            execution_stack: Default::default(),
            execution_engine_v1: ExecutionEngineV1::new(EngineConfig::default()), /* TODO: Don't
                                                                                   * use default
                                                                                   * instance. */
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

    pub fn execute_wasm_v2_request<P>(
        &self,
        state_root_hash: Digest,
        state_provider: &P,
        request: WasmV2Request,
    ) -> Result<WasmV2Result, WasmV2Error>
    where
        P: StateProvider + CommitProvider,
        <P as StateProvider>::Reader: 'static,
    {
        match request {
            WasmV2Request::Install(install_request) => {
                match self.install_contract(state_root_hash, state_provider, install_request) {
                    Ok(result) => Ok(WasmV2Result::Install(result)),
                    Err(error) => Err(WasmV2Error::Install(error)),
                }
            }
            WasmV2Request::Execute(execute_request) => {
                match self.execute_with_provider(state_root_hash, state_provider, execute_request) {
                    Ok(result) => Ok(WasmV2Result::Execute(result)),
                    Err(error) => Err(WasmV2Error::Execute(error)),
                }
            }
        }
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
        StoredValue::Account(account) => account.main_purse(),
        other => panic!("should be account or contract received {other:?}"),
    }
}

/// The request to execute a Wasm contract.
pub enum WasmV2Request {
    /// The request to install a Wasm contract.
    Install(InstallContractRequest),
    /// The request to execute a Wasm contract.
    Execute(ExecuteRequest),
}

/// The result of executing a Wasm contract.
pub enum WasmV2Result {
    /// The result of installing a Wasm contract.
    Install(InstallContractResult),
    /// The result of executing a Wasm contract.
    Execute(ExecuteWithProviderResult),
}

impl WasmV2Result {
    /// Returns the state root hash after the contract execution.
    pub fn state_root_hash(&self) -> Digest {
        match self {
            WasmV2Result::Install(result) => result.post_state_hash,
            WasmV2Result::Execute(result) => result.post_state_hash,
        }
    }

    /// Returns the gas usage of the contract execution.
    pub fn gas_usage(&self) -> &GasUsage {
        match self {
            WasmV2Result::Install(result) => &result.gas_usage,
            WasmV2Result::Execute(result) => &result.gas_usage,
        }
    }

    /// Returns the effects of the contract execution.
    pub fn effects(&self) -> &Effects {
        match self {
            WasmV2Result::Install(result) => &result.effects,
            WasmV2Result::Execute(result) => &result.effects,
        }
    }
}

#[derive(Error, Debug)]
pub enum WasmV2Error {
    #[error(transparent)]
    Install(InstallContractError),
    #[error(transparent)]
    Execute(ExecuteWithProviderError),
}

#[derive(Clone, Eq, PartialEq, Error, Debug)]
pub enum InvalidRequest {
    #[error("Invalid transaction variant")]
    InvalidTransactionVariant,
    #[error("Invalid gas limit: {0}")]
    InvalidGasLimit(U512),
}

impl WasmV2Request {
    pub fn new(
        gas_limit: Gas,
        network_name: impl Into<Arc<str>>,
        transaction: &Transaction,
    ) -> Result<Self, InvalidRequest> {
        // don't panic, return error that similar to V1Request
        let transaction_v1 = match transaction.as_transaction_v1() {
            Some(transaction_v1) => transaction_v1,
            None => {
                return Err(InvalidRequest::InvalidTransactionVariant);
            }
        };

        let transaction_hash = transaction.hash();
        let initiator_addr = transaction.initiator_addr();

        let gas_limit: u64 = gas_limit
            .value()
            .try_into()
            .map_err(|_| InvalidRequest::InvalidGasLimit(gas_limit.value()))?;

        let address_generator = AddressGeneratorBuilder::default()
            .seed_with(transaction_hash.as_ref())
            .build();

        // If it's wrong args variant => invalid request => penalty payment
        let input_data = transaction_v1.body().args().clone().into_bytesrepr(); // TODO: Make non optional
        let value = transaction_v1.body().value();

        enum Target {
            Install {
                module_bytes: Bytes,
                entry_point: String,
            },
            Session {
                module_bytes: Bytes,
            },
            Stored {
                id: TransactionInvocationTarget,
                entry_point: String,
            },
        }

        let target = match transaction_v1.body().target() {
            TransactionTarget::Native => todo!(), //
            TransactionTarget::Stored { id, runtime: _ } => {
                match transaction_v1.body().entry_point() {
                    TransactionEntryPoint::Custom(entry_point) => Target::Stored {
                        id: id.clone(),
                        entry_point: entry_point.clone(),
                    },
                    _ => todo!(),
                }
            }
            TransactionTarget::Session {
                module_bytes,
                runtime: _,
            } => match transaction_v1.body().entry_point() {
                TransactionEntryPoint::Call => Target::Session {
                    module_bytes: module_bytes.clone().take_inner().into(),
                },
                TransactionEntryPoint::Custom(entry_point) => Target::Install {
                    module_bytes: module_bytes.clone().take_inner().into(),
                    entry_point: entry_point.to_string(),
                },
                _ => todo!(),
            },
        };

        info!(%transaction_hash, "executing v1 contract");

        match target {
            Target::Install {
                module_bytes,
                entry_point,
            } => {
                let mut builder = InstallContractRequestBuilder::default();

                let entry_point = (!entry_point.is_empty()).then_some(entry_point);

                match entry_point {
                    Some(entry_point) => {
                        builder = builder.with_entry_point(entry_point.clone());

                        if let Some(input_data) = input_data {
                            builder = builder.with_input(input_data.take_inner().into());
                        }
                    }
                    None => {
                        assert!(
                            input_data.is_none()
                                || matches!(input_data, Some(input_data) if input_data.is_empty())
                        );
                    }
                }

                let install_request = builder
                    .with_initiator(initiator_addr.account_hash())
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_wasm_bytes(module_bytes)
                    .with_address_generator(address_generator)
                    .with_transferred_value(value)
                    .with_chain_name(network_name)
                    .with_block_time(transaction.timestamp())
                    .build()
                    .expect("should build");

                Ok(Self::Install(install_request))
            }
            Target::Session { .. } | Target::Stored { .. } => {
                let mut builder = ExecuteRequestBuilder::default();

                let initiator_account_hash = &initiator_addr.account_hash();

                let initiator_key = Key::Account(*initiator_account_hash);

                builder = builder
                    .with_address_generator(address_generator)
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_initiator(*initiator_account_hash)
                    .with_caller_key(initiator_key)
                    // TODO: Callee is unnecessary as it can be derived from the
                    // execution target inside the executor
                    .with_callee_key(initiator_key)
                    .with_chain_name(network_name)
                    .with_transferred_value(value)
                    .with_block_time(transaction.timestamp());

                if let Some(input_data) = input_data.clone() {
                    builder = builder.with_input(input_data.clone().take_inner().into());
                }

                let execution_kind = match target {
                    Target::Session { module_bytes } => ExecutionKind::SessionBytes(module_bytes),
                    Target::Stored {
                        id: TransactionInvocationTarget::ByHash(address),
                        entry_point,
                    } => ExecutionKind::Stored {
                        address: EntityAddr::SmartContract(address),
                        entry_point: entry_point.clone(),
                    },
                    Target::Stored { id, entry_point } => {
                        todo!("Unsupported target {entry_point} {id:?}")
                    }
                    Target::Install { .. } => unreachable!(),
                };

                builder = builder.with_target(execution_kind);

                let execute_request = builder.build().expect("should build");

                Ok(Self::Execute(execute_request))
            }
        }
    }
}
