use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use casper_storage::global_state::state::StateReader;
use casper_types::{
    account::AccountHash,
    addressable_entity::NamedKeys,
    bytesrepr::FromBytes,
    package::ContractPackageKind,
    system::{auction, handle_payment, mint, AUCTION, HANDLE_PAYMENT, MINT},
    AddressableEntity, BlockTime, CLTyped, ContextAccessRights, ContractHash, DeployHash,
    EntryPointType, Gas, Key, Phase, ProtocolVersion, RuntimeArgs, StoredValue, U512,
};

use crate::{
    engine_state::{execution_kind::ExecutionKind, EngineConfig, ExecutionResult},
    execution::{address_generator::AddressGenerator, Error},
    runtime::{Runtime, RuntimeStack},
    runtime_context::RuntimeContext,
    tracking_copy::{TrackingCopy, TrackingCopyExt},
};

const ARG_AMOUNT: &str = "amount";

fn try_get_amount(runtime_args: &RuntimeArgs) -> Result<U512, Error> {
    runtime_args.try_get_number(ARG_AMOUNT).map_err(Error::from)
}

/// Executor object deals with execution of WASM modules.
pub struct Executor {
    config: EngineConfig,
}

impl Executor {
    /// Creates new executor object.
    pub fn new(config: EngineConfig) -> Self {
        Executor { config }
    }

    /// Executes a WASM module.
    ///
    /// This method checks if a given contract hash is a system contract, and then short circuits to
    /// a specific native implementation of it. Otherwise, a supplied WASM module is executed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn exec<R>(
        &self,
        execution_kind: ExecutionKind,
        args: RuntimeArgs,
        contract_hash: ContractHash,
        entity: &AddressableEntity,
        package_kind: ContractPackageKind,
        named_keys: &mut NamedKeys,
        access_rights: ContextAccessRights,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
    ) -> ExecutionResult
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        let spending_limit: U512 = match try_get_amount(&args) {
            Ok(spending_limit) => spending_limit,
            Err(error) => {
                return ExecutionResult::precondition_failure(error.into());
            }
        };

        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };

        let context = self.create_runtime_context(
            named_keys,
            entity,
            contract_hash,
            authorization_keys,
            access_rights,
            package_kind,
            account_hash,
            address_generator,
            tracking_copy,
            blocktime,
            protocol_version,
            deploy_hash,
            phase,
            args.clone(),
            gas_limit,
            spending_limit,
            EntryPointType::Session,
        );

        let mut runtime = Runtime::new(context);

        let result = match execution_kind {
            ExecutionKind::Module(module_bytes) => {
                runtime.execute_module_bytes(&module_bytes, stack)
            }
            ExecutionKind::Contract {
                contract_hash,
                entry_point_name,
            } => {
                // These args are passed through here as they are required to construct the new
                // `Runtime` during the contract's execution (i.e. inside
                // `Runtime::execute_contract`).
                match runtime.migrate_contract_and_contract_package(contract_hash) {
                    Ok(()) => runtime.call_contract_with_stack(
                        contract_hash,
                        &entry_point_name,
                        args,
                        stack,
                    ),
                    Err(error) => Err(error),
                }
            }
        };

        match result {
            Ok(_) => ExecutionResult::Success {
                effects: runtime.context().effects(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
            Err(error) => ExecutionResult::Failure {
                error: error.into(),
                effects: runtime.context().effects(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
        }
    }

    /// Executes standard payment code natively.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn exec_standard_payment<R>(
        &self,
        payment_args: RuntimeArgs,
        payment_base_key: ContractHash,
        entity: &AddressableEntity,
        package_kind: ContractPackageKind,
        payment_named_keys: &mut NamedKeys,
        access_rights: ContextAccessRights,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        payment_gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
    ) -> ExecutionResult
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        let spending_limit: U512 = match try_get_amount(&payment_args) {
            Ok(spending_limit) => spending_limit,
            Err(error) => {
                return ExecutionResult::precondition_failure(error.into());
            }
        };

        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };

        let runtime_context = self.create_runtime_context(
            payment_named_keys,
            entity,
            payment_base_key,
            authorization_keys,
            access_rights,
            package_kind,
            account_hash,
            address_generator,
            Rc::clone(&tracking_copy),
            blocktime,
            protocol_version,
            deploy_hash,
            phase,
            payment_args,
            payment_gas_limit,
            spending_limit,
            EntryPointType::Session,
        );

        let effects = tracking_copy.borrow().effects();

        // Standard payment is executed in the calling account's context; the stack already
        // captures that.
        let mut runtime = Runtime::new(runtime_context);

        match runtime.call_host_standard_payment(stack) {
            Ok(()) => ExecutionResult::Success {
                effects: runtime.context().effects(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
            Err(error) => ExecutionResult::Failure {
                effects,
                error: error.into(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            },
        }
    }

    /// Handles necessary address resolution and orchestration to securely call a system contract
    /// using the runtime.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_system_contract<R, T>(
        &self,
        direct_system_contract_call: DirectSystemContractCall,
        runtime_args: RuntimeArgs,
        entity: &AddressableEntity,
        package_kind: ContractPackageKind,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
        remaining_spending_limit: U512,
    ) -> (Option<T>, ExecutionResult)
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
        T: FromBytes + CLTyped,
    {
        let address_generator = {
            let generator = AddressGenerator::new(deploy_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };

        // Today lack of existence of the system contract registry and lack of entry
        // for the minimum defined system contracts (mint, auction, handle_payment)
        // should cause the EE to panic. Do not remove the panics.
        let system_contract_registry = tracking_copy
            .borrow_mut()
            .get_system_contracts()
            .unwrap_or_else(|error| panic!("Could not retrieve system contracts: {:?}", error));

        // Snapshot of effects before execution, so in case of error only nonce update
        // can be returned.
        let effects = tracking_copy.borrow().effects();

        let entry_point_name = direct_system_contract_call.entry_point_name();

        let contract_hash = match direct_system_contract_call {
            DirectSystemContractCall::Slash
            | DirectSystemContractCall::RunAuction
            | DirectSystemContractCall::DistributeRewards => {
                let auction_hash = system_contract_registry
                    .get(AUCTION)
                    .expect("should have auction hash");
                *auction_hash
            }
            DirectSystemContractCall::CreatePurse | DirectSystemContractCall::Transfer => {
                let mint_hash = system_contract_registry
                    .get(MINT)
                    .expect("should have mint hash");
                *mint_hash
            }
            DirectSystemContractCall::FinalizePayment
            | DirectSystemContractCall::GetPaymentPurse
            | DirectSystemContractCall::DistributeAccumulatedFees => {
                let handle_payment_hash = system_contract_registry
                    .get(HANDLE_PAYMENT)
                    .expect("should have handle payment");
                *handle_payment_hash
            }
        };

        let contract = match tracking_copy.borrow_mut().get_contract(contract_hash) {
            Ok(contract) => contract,
            Err(error) => return (None, ExecutionResult::precondition_failure(error.into())),
        };

        let mut named_keys = contract.named_keys().clone();
        let access_rights = contract.extract_access_rights(contract_hash);

        let runtime_context = self.create_runtime_context(
            &mut named_keys,
            entity,
            contract_hash,
            authorization_keys,
            access_rights,
            package_kind,
            account_hash,
            address_generator,
            tracking_copy,
            blocktime,
            protocol_version,
            deploy_hash,
            phase,
            runtime_args.clone(),
            gas_limit,
            remaining_spending_limit,
            EntryPointType::Contract,
        );

        let mut runtime = Runtime::new(runtime_context);

        // DO NOT alter this logic to call a system contract directly (such as via mint_internal,
        // etc). Doing so would bypass necessary context based security checks in some use cases. It
        // is intentional to use the runtime machinery for this interaction with the system
        // contracts, to force all such security checks for usage via the executor into a single
        // execution path.
        let result =
            runtime.call_contract_with_stack(contract_hash, entry_point_name, runtime_args, stack);

        match result {
            Ok(value) => match value.into_t() {
                Ok(ret) => ExecutionResult::Success {
                    effects: runtime.context().effects(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                }
                .take_with_ret(ret),
                Err(error) => ExecutionResult::Failure {
                    effects,
                    error: Error::CLValue(error).into(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                }
                .take_without_ret(),
            },
            Err(error) => ExecutionResult::Failure {
                effects,
                error: error.into(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
            }
            .take_without_ret(),
        }
    }

    /// Creates new runtime context.
    #[allow(clippy::too_many_arguments)]
    fn create_runtime_context<'a, R>(
        &self,
        named_keys: &'a mut NamedKeys,
        entity: &'a AddressableEntity,
        entity_address: ContractHash,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        package_kind: ContractPackageKind,
        account_hash: AccountHash,
        address_generator: Rc<RefCell<AddressGenerator>>,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        blocktime: BlockTime,
        protocol_version: ProtocolVersion,
        deploy_hash: DeployHash,
        phase: Phase,
        runtime_args: RuntimeArgs,
        gas_limit: Gas,
        remaining_spending_limit: U512,
        entry_point_type: EntryPointType,
    ) -> RuntimeContext<'a, R>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<Error>,
    {
        let gas_counter = Gas::default();
        let transfers = Vec::default();

        RuntimeContext::new(
            named_keys,
            entity,
            entity_address,
            authorization_keys,
            access_rights,
            package_kind,
            account_hash,
            address_generator,
            tracking_copy,
            self.config.clone(),
            blocktime,
            protocol_version,
            deploy_hash,
            phase,
            runtime_args,
            gas_limit,
            gas_counter,
            transfers,
            remaining_spending_limit,
            entry_point_type,
        )
    }
}

/// Represents a variant of a system contract call.
pub(crate) enum DirectSystemContractCall {
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
    /// Calls handle payment's `get_payment_purse` entry point.
    GetPaymentPurse,
    /// Calls handle payment's `distribute_accumulated_fees` entry point.
    DistributeAccumulatedFees,
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
            DirectSystemContractCall::GetPaymentPurse => handle_payment::METHOD_GET_PAYMENT_PURSE,
            DirectSystemContractCall::DistributeAccumulatedFees => {
                handle_payment::METHOD_DISTRIBUTE_ACCUMULATED_FEES
            }
        }
    }
}
