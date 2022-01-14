use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use parity_wasm::elements::Module;
use tracing::warn;
use wasmi::ModuleRef;

use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::FromBytes,
    contracts::NamedKeys,
    system::{auction, handle_payment, mint, AUCTION, HANDLE_PAYMENT, MINT},
    BlockTime, CLTyped, ContractPackage, DeployHash, EntryPoint, EntryPointType, Gas, Key, Phase,
    ProtocolVersion, RuntimeArgs, StoredValue,
};

use crate::{
    core::{
        engine_state::{execution_result::ExecutionResult, EngineConfig, SystemContractRegistry},
        execution::{address_generator::AddressGenerator, Error},
        runtime::{extract_access_rights_from_keys, instance_and_memory, Runtime, RuntimeStack},
        runtime_context::{self, RuntimeContext},
        tracking_copy::TrackingCopy,
    },
    shared::{execution_journal::ExecutionJournal, newtypes::CorrelationId},
    storage::global_state::StateReader,
};

macro_rules! on_fail_charge {
    ($fn:expr) => {
        match $fn {
            Ok(res) => res,
            Err(e) => {
                let exec_err: Error = e.into();
                warn!("Execution failed: {:?}", exec_err);
                return ExecutionResult::precondition_failure(exec_err.into());
            }
        }
    };
    ($fn:expr, $cost:expr, $transfers:expr) => {
        match $fn {
            Ok(res) => res,
            Err(e) => {
                let exec_err: Error = e.into();
                warn!("Execution failed: {:?}", exec_err);
                return ExecutionResult::Failure {
                    error: exec_err.into(),
                    execution_journal: Default::default(),
                    transfers: $transfers,
                    cost: $cost,
                };
            }
        }
    };
    ($fn:expr, $cost:expr, $execution_journal:expr, $transfers:expr) => {
        match $fn {
            Ok(res) => res,
            Err(e) => {
                let exec_err: Error = e.into();
                warn!("Execution failed: {:?}", exec_err);
                return ExecutionResult::Failure {
                    error: exec_err.into(),
                    execution_journal: $execution_journal,
                    transfers: $transfers,
                    cost: $cost,
                };
            }
        }
    };
}

/// Executor object deals with execution of WASM modules.
pub struct Executor {
    config: EngineConfig,
}

#[allow(clippy::too_many_arguments)]
impl Executor {
    /// Creates new executor object.
    pub fn new(config: EngineConfig) -> Self {
        Executor { config }
    }

    /// Returns config.
    pub fn config(&self) -> EngineConfig {
        self.config
    }

    /// Executes a WASM module.
    ///
    /// This method checks if a given contract hash is a system contract, and then short circuits to
    /// a specific native implementation of it. Otherwise, a supplied WASM module is executed.
    #[allow(clippy::too_many_arguments)]
    pub fn exec<R>(
        &self,
        module: Module,
        entry_point: EntryPoint,
        args: RuntimeArgs,
        base_key: Key,
        account: &Account,
        named_keys: &mut NamedKeys,
        authorization_keys: BTreeSet<AccountHash>,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        protocol_version: ProtocolVersion,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        contract_package: &ContractPackage,
        stack: RuntimeStack,
        system_contract_registry: SystemContractRegistry,
    ) -> ExecutionResult
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        let entry_point_name = entry_point.name();
        let entry_point_type = entry_point.entry_point_type();
        let entry_point_access = entry_point.access();

        let (instance, memory) = on_fail_charge!(instance_and_memory(
            module.clone(),
            protocol_version,
            self.config.wasm_config()
        ));

        let access_rights = {
            let keys: Vec<Key> = named_keys.values().cloned().collect();
            extract_access_rights_from_keys(keys)
        };

        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_bytes(), phase);
            Rc::new(RefCell::new(generator))
        };
        let gas_counter: Gas = Gas::default();
        let transfers = Vec::default();

        // Snapshot of the journal before execution, so in case of an error
        // we don't apply the execution changes but keep the payment code effects.
        let execution_journal = tracking_copy.borrow().execution_journal();

        let context = RuntimeContext::new(
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args.clone(),
            authorization_keys,
            account,
            base_key,
            blocktime,
            deploy_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            correlation_id,
            phase,
            self.config,
            transfers,
            system_contract_registry,
        );

        let mut runtime = Runtime::new(self.config, memory, module, context, stack);

        let accounts_access_rights = {
            let keys: Vec<Key> = account.named_keys().values().cloned().collect();
            extract_access_rights_from_keys(keys)
        };

        on_fail_charge!(runtime_context::validate_entry_point_access_with(
            contract_package,
            entry_point_access,
            |uref| runtime_context::uref_has_access_rights(uref, &accounts_access_rights)
        ));

        let stack = runtime.stack().clone();

        if runtime.is_mint(base_key) {
            match runtime.call_host_mint(
                protocol_version,
                entry_point.name(),
                &args,
                Default::default(),
                stack,
            ) {
                Ok(_value) => {
                    return ExecutionResult::Success {
                        execution_journal: runtime.context().execution_journal(),
                        transfers: runtime.context().transfers().to_owned(),
                        cost: runtime.context().gas_counter(),
                    };
                }
                Err(error) => {
                    return ExecutionResult::Failure {
                        error: error.into(),
                        execution_journal,
                        transfers: runtime.context().transfers().to_owned(),
                        cost: runtime.context().gas_counter(),
                    };
                }
            }
        } else if runtime.is_handle_payment(base_key) {
            match runtime.call_host_handle_payment(
                protocol_version,
                entry_point.name(),
                &args,
                Default::default(),
                stack,
            ) {
                Ok(_value) => {
                    return ExecutionResult::Success {
                        execution_journal: runtime.context().execution_journal(),
                        transfers: runtime.context().transfers().to_owned(),
                        cost: runtime.context().gas_counter(),
                    };
                }
                Err(error) => {
                    return ExecutionResult::Failure {
                        error: error.into(),
                        transfers: runtime.context().transfers().to_owned(),
                        execution_journal,
                        cost: runtime.context().gas_counter(),
                    };
                }
            }
        } else if runtime.is_auction(base_key) {
            match runtime.call_host_auction(
                protocol_version,
                entry_point.name(),
                &args,
                Default::default(),
                stack,
            ) {
                Ok(_value) => {
                    return ExecutionResult::Success {
                        execution_journal: runtime.context().execution_journal(),
                        transfers: runtime.context().transfers().to_owned(),
                        cost: runtime.context().gas_counter(),
                    }
                }
                Err(error) => {
                    return ExecutionResult::Failure {
                        execution_journal,
                        error: error.into(),
                        transfers: runtime.context().transfers().to_owned(),
                        cost: runtime.context().gas_counter(),
                    }
                }
            }
        }
        on_fail_charge!(
            instance.invoke_export(entry_point_name, &[], &mut runtime),
            runtime.context().gas_counter(),
            execution_journal,
            runtime.context().transfers().to_owned()
        );

        ExecutionResult::Success {
            execution_journal: runtime.context().execution_journal(),
            transfers: runtime.context().transfers().to_owned(),
            cost: runtime.context().gas_counter(),
        }
    }

    /// Executes a standard payment code natively.
    pub fn exec_standard_payment<R>(
        &self,
        system_module: Module,
        payment_args: RuntimeArgs,
        payment_base_key: Key,
        account: &Account,
        payment_named_keys: &mut NamedKeys,
        authorization_keys: BTreeSet<AccountHash>,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        payment_gas_limit: Gas,
        protocol_version: ProtocolVersion,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
        system_contract_registry: SystemContractRegistry,
    ) -> ExecutionResult
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        // use host side standard payment
        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_bytes(), phase);
            Rc::new(RefCell::new(generator))
        };

        let mut runtime = match self.create_runtime(
            system_module,
            EntryPointType::Session,
            payment_args,
            payment_named_keys,
            Default::default(),
            payment_base_key,
            account,
            authorization_keys,
            blocktime,
            deploy_hash,
            payment_gas_limit,
            address_generator,
            protocol_version,
            correlation_id,
            Rc::clone(&tracking_copy),
            phase,
            stack,
            system_contract_registry,
        ) {
            Ok((_instance, runtime)) => runtime,
            Err(error) => {
                return ExecutionResult::Failure {
                    error: error.into(),
                    execution_journal: Default::default(),
                    transfers: Vec::default(),
                    cost: Gas::default(),
                };
            }
        };

        let execution_journal = tracking_copy.borrow().execution_journal();

        match runtime.call_host_standard_payment() {
            Ok(()) => ExecutionResult::Success {
                execution_journal: runtime.context().execution_journal(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
            Err(error) => ExecutionResult::Failure {
                execution_journal,
                error: error.into(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
        }
    }

    /// Executes a system contract.
    ///
    /// System contracts are implemented as a native code, and no WASM execution is involved at all.
    /// This approach has a benefit of speed as compared to executing WASM modules.
    ///
    /// Returns an optional return value from the system contract call, and an [`ExecutionResult`].
    pub fn exec_system_contract<R, T>(
        &self,
        direct_system_contract_call: DirectSystemContractCall,
        module: Module,
        runtime_args: RuntimeArgs,
        named_keys: &mut NamedKeys,
        extra_keys: &[Key],
        base_key: Key,
        account: &Account,
        authorization_keys: BTreeSet<AccountHash>,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        protocol_version: ProtocolVersion,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
        system_contract_registry: SystemContractRegistry,
    ) -> (Option<T>, ExecutionResult)
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
        T: FromBytes + CLTyped,
    {
        match direct_system_contract_call {
            DirectSystemContractCall::Slash
            | DirectSystemContractCall::RunAuction
            | DirectSystemContractCall::DistributeRewards => {
                // TODO See if these panics can be removed.
                let auction_hash = system_contract_registry
                    .get(AUCTION)
                    .expect("should have auction hash")
                    .to_owned();

                if Some(auction_hash.value()) != base_key.into_hash() {
                    panic!(
                        "{} should only be called with the auction contract",
                        direct_system_contract_call.entry_point_name()
                    );
                }
            }
            DirectSystemContractCall::FinalizePayment
            | DirectSystemContractCall::GetPaymentPurse => {
                // TODO See if these panics can be removed.
                let handle_payment = system_contract_registry
                    .get(HANDLE_PAYMENT)
                    .expect("should have handle payment");
                if Some(handle_payment.value()) != base_key.into_hash() {
                    panic!(
                        "{} should only be called with the handle payment contract",
                        direct_system_contract_call.entry_point_name()
                    );
                }
            }
            DirectSystemContractCall::CreatePurse | DirectSystemContractCall::Transfer => {
                // TODO See if these panics can be removed.
                let mint_hash = system_contract_registry
                    .get(MINT)
                    .expect("should have mint hash");
                if Some(mint_hash.value()) != base_key.into_hash() {
                    panic!(
                        "{} should only be called with the mint contract",
                        direct_system_contract_call.entry_point_name()
                    );
                }
            }
            DirectSystemContractCall::GetEraValidators => {
                // TODO See if these panics can be removed.
                let auction_hash = system_contract_registry
                    .get(AUCTION)
                    .expect("should have auction hash")
                    .to_owned();
                if Some(auction_hash.value()) != base_key.into_hash() {
                    panic!(
                        "{} should only be called with the auction contract",
                        direct_system_contract_call.entry_point_name()
                    );
                }
            }
        }

        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_bytes(), phase);
            Rc::new(RefCell::new(generator))
        };
        let gas_counter = Gas::default(); // maybe const?

        // Snapshot of effects before execution, so in case of error only nonce update
        // can be returned.
        let execution_journal = tracking_copy.borrow().execution_journal();

        let transfers = Vec::default();

        let (_, runtime) = match self.create_runtime(
            module,
            EntryPointType::Contract,
            runtime_args.clone(),
            named_keys,
            extra_keys,
            base_key,
            account,
            authorization_keys,
            blocktime,
            deploy_hash,
            gas_limit,
            address_generator,
            protocol_version,
            correlation_id,
            tracking_copy,
            phase,
            stack,
            system_contract_registry,
        ) {
            Ok((instance, runtime)) => (instance, runtime),
            Err(error) => {
                return ExecutionResult::Failure {
                    execution_journal,
                    transfers,
                    cost: gas_counter,
                    error: error.into(),
                }
                .take_without_ret()
            }
        };

        let inner_named_keys = runtime.context().named_keys().clone();
        let ret = direct_system_contract_call.host_exec(
            runtime,
            protocol_version,
            &runtime_args,
            extra_keys,
            execution_journal,
        );
        *named_keys = inner_named_keys;
        ret
    }

    /// Creates new runtime object.
    ///
    /// This method also deals with proper initialiation of a WASM module by pre-allocating a memory
    /// instance, and attaching a host function resolver.
    ///
    /// Returns a module and an instance of [`Runtime`] which is ready to execute the WASM modules.
    pub(crate) fn create_runtime<'a, R>(
        &self,
        module: Module,
        entry_point_type: EntryPointType,
        runtime_args: RuntimeArgs,
        named_keys: &'a mut NamedKeys,
        extra_keys: &[Key],
        base_key: Key,
        account: &'a Account,
        authorization_keys: BTreeSet<AccountHash>,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        address_generator: Rc<RefCell<AddressGenerator>>,
        protocol_version: ProtocolVersion,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
        system_contract_registry: SystemContractRegistry,
    ) -> Result<(ModuleRef, Runtime<'a, R>), Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        let access_rights = {
            let mut keys: Vec<Key> = named_keys.values().cloned().collect();
            keys.extend(extra_keys);
            extract_access_rights_from_keys(keys)
        };

        let gas_counter = Gas::default();
        let transfers = Vec::default();

        let runtime_context = RuntimeContext::new(
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            runtime_args,
            authorization_keys,
            account,
            base_key,
            blocktime,
            deploy_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            correlation_id,
            phase,
            self.config,
            transfers,
            system_contract_registry,
        );

        let (instance, memory) =
            instance_and_memory(module.clone(), protocol_version, self.config.wasm_config())?;

        let runtime = Runtime::new(self.config, memory, module, runtime_context, stack);

        Ok((instance, runtime))
    }
}

/// Represents a variant of a system contract call.
pub enum DirectSystemContractCall {
    /// Calls auction's `slash` entry point.
    Slash,
    /// Calls auction's `run_auction` entry point.
    RunAuction,
    /// Calls auction's `distribute` entry point.
    DistributeRewards,
    /// Calls handle payment's `finalize` entry point.
    FinalizePayment,
    /// Calls mint's `create` entry point.
    CreatePurse,
    /// Calls mint's `transfer` entry point.
    Transfer,
    /// Calls auction's `get_era_validators` entry point.
    GetEraValidators,
    /// Calls handle payment's `
    GetPaymentPurse,
}

impl DirectSystemContractCall {
    fn entry_point_name(&self) -> &str {
        match self {
            DirectSystemContractCall::Slash => auction::METHOD_SLASH,
            DirectSystemContractCall::RunAuction => auction::METHOD_RUN_AUCTION,
            DirectSystemContractCall::DistributeRewards => auction::METHOD_DISTRIBUTE,
            DirectSystemContractCall::FinalizePayment => handle_payment::METHOD_FINALIZE_PAYMENT,
            DirectSystemContractCall::CreatePurse => mint::METHOD_CREATE,
            DirectSystemContractCall::Transfer => mint::METHOD_TRANSFER,
            DirectSystemContractCall::GetEraValidators => auction::METHOD_GET_ERA_VALIDATORS,
            DirectSystemContractCall::GetPaymentPurse => handle_payment::METHOD_GET_PAYMENT_PURSE,
        }
    }

    fn host_exec<R, T>(
        &self,
        mut runtime: Runtime<R>,
        protocol_version: ProtocolVersion,
        runtime_args: &RuntimeArgs,
        extra_keys: &[Key],
        execution_journal: ExecutionJournal,
    ) -> (Option<T>, ExecutionResult)
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
        T: FromBytes + CLTyped,
    {
        let entry_point_name = self.entry_point_name();

        let stack = runtime.stack().clone();

        let result = match self {
            DirectSystemContractCall::Slash
            | DirectSystemContractCall::RunAuction
            | DirectSystemContractCall::DistributeRewards => runtime.call_host_auction(
                protocol_version,
                entry_point_name,
                runtime_args,
                extra_keys,
                stack,
            ),
            DirectSystemContractCall::FinalizePayment => runtime.call_host_handle_payment(
                protocol_version,
                entry_point_name,
                runtime_args,
                extra_keys,
                stack,
            ),
            DirectSystemContractCall::CreatePurse | DirectSystemContractCall::Transfer => runtime
                .call_host_mint(
                    protocol_version,
                    entry_point_name,
                    runtime_args,
                    extra_keys,
                    stack,
                ),
            DirectSystemContractCall::GetEraValidators => runtime.call_host_auction(
                protocol_version,
                entry_point_name,
                runtime_args,
                extra_keys,
                stack,
            ),

            DirectSystemContractCall::GetPaymentPurse => runtime.call_host_handle_payment(
                protocol_version,
                entry_point_name,
                runtime_args,
                extra_keys,
                stack,
            ),
        };

        match result {
            Ok(value) => match value.into_t() {
                Ok(ret) => ExecutionResult::Success {
                    execution_journal: runtime.context().execution_journal(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                }
                .take_with_ret(ret),
                Err(error) => ExecutionResult::Failure {
                    execution_journal,
                    error: Error::CLValue(error).into(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                }
                .take_without_ret(),
            },
            Err(error) => ExecutionResult::Failure {
                execution_journal,
                error: error.into(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            }
            .take_without_ret(),
        }
    }
}
