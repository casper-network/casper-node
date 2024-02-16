use std::{cell::RefCell, collections::BTreeSet, convert::TryFrom, rc::Rc};

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyExt},
    AddressGenerator,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{EntityKind, NamedKeys},
    bytesrepr::FromBytes,
    system::{auction, handle_payment, mint, AUCTION, HANDLE_PAYMENT, MINT},
    AddressableEntity, AddressableEntityHash, ApiError, BlockTime, CLTyped, ContextAccessRights,
    DeployHash, EntityAddr, EntryPointType, Gas, Key, Phase, ProtocolVersion, RuntimeArgs,
    StoredValue, Tagged, URef, U512,
};

use crate::engine_state::TransferArgs;

use crate::{
    engine_state::{
        execution_kind::ExecutionKind, EngineConfig, Error as EngineStateError, ExecutionResult,
    },
    execution::Error,
    runtime::{Runtime, RuntimeStack},
    runtime_context::RuntimeContext,
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
        entity_hash: AddressableEntityHash,
        entity: &AddressableEntity,
        entity_kind: EntityKind,
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
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
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

        let entity_key = Key::addressable_entity_key(entity_kind.tag(), entity_hash);

        let context = self.create_runtime_context(
            named_keys,
            entity,
            entity_key,
            authorization_keys,
            access_rights,
            entity_kind,
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
                entity_hash: contract_hash,
                entry_point_name,
            } => {
                // These args are passed through here as they are required to construct the new
                // `Runtime` during the contract's execution (i.e. inside
                // `Runtime::execute_contract`).
                runtime.call_contract_with_stack(contract_hash, &entry_point_name, args, stack)
            }
        };

        match result {
            Ok(_) => ExecutionResult::Success {
                effects: runtime.context().effects(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
                messages: runtime.context().messages(),
            },
            Err(error) => ExecutionResult::Failure {
                error: error.into(),
                effects: runtime.context().effects(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
                messages: runtime.context().messages(),
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
        entity_kind: EntityKind,
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
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
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
        let messages = tracking_copy.borrow().messages();

        let entry_point_name = direct_system_contract_call.entry_point_name();

        let entity_hash = match direct_system_contract_call {
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

        let contract = match tracking_copy
            .borrow_mut()
            .get_addressable_entity(entity_hash)
        {
            Ok(contract) => contract,
            Err(error) => return (None, ExecutionResult::precondition_failure(error.into())),
        };

        let entity_addr = EntityAddr::new_with_tag(entity_kind, entity_hash.value());

        let mut named_keys = match tracking_copy.borrow_mut().get_named_keys(entity_addr) {
            Ok(named_key) => named_key,
            Err(error) => return (None, ExecutionResult::precondition_failure(error.into())),
        };

        let access_rights = contract.extract_access_rights(entity_hash, &named_keys);
        let entity_address = entity_addr.into();
        let runtime_context = self.create_runtime_context(
            &mut named_keys,
            entity,
            entity_address,
            authorization_keys,
            access_rights,
            entity_kind,
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
            EntryPointType::AddressableEntity,
        );

        let mut runtime = Runtime::new(runtime_context);

        // DO NOT alter this logic to call a system contract directly (such as via mint_internal,
        // etc). Doing so would bypass necessary context based security checks in some use cases. It
        // is intentional to use the runtime machinery for this interaction with the system
        // contracts, to force all such security checks for usage via the executor into a single
        // execution path.
        let result =
            runtime.call_contract_with_stack(entity_hash, entry_point_name, runtime_args, stack);

        match result {
            Ok(value) => match value.into_t() {
                Ok(ret) => ExecutionResult::Success {
                    effects: runtime.context().effects(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                    messages: runtime.context().messages(),
                }
                .take_with_ret(ret),
                Err(error) => ExecutionResult::Failure {
                    effects,
                    error: Error::CLValue(error).into(),
                    transfers: runtime.context().transfers().to_owned(),
                    cost: runtime.context().gas_counter(),
                    messages,
                }
                .take_without_ret(),
            },
            Err(error) => ExecutionResult::Failure {
                effects,
                error: error.into(),
                transfers: runtime.context().transfers().to_owned(),
                cost: runtime.context().gas_counter(),
                messages,
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
        entity_key: Key,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        entity_kind: EntityKind,
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
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let gas_counter = Gas::default();
        let transfers = Vec::default();

        RuntimeContext::new(
            named_keys,
            entity,
            entity_key,
            authorization_keys,
            access_rights,
            entity_kind,
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

    /// Executes standard payment code natively.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn exec_standard_payment<R>(
        &self,
        payment_args: RuntimeArgs,
        entity: &AddressableEntity,
        entity_kind: EntityKind,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        payment_gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        max_stack_height: usize,
    ) -> Result<ExecutionResult, EngineStateError>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
        R::Error: Into<Error>,
    {
        let payment_amount: U512 = match try_get_amount(&payment_args) {
            Ok(payment_amount) => payment_amount,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        let get_payment_purse_stack = RuntimeStack::new_system_call_stack(max_stack_height);

        let (maybe_purse, get_payment_result) = self.get_payment_purse(
            entity,
            entity_kind,
            authorization_keys.clone(),
            account_hash,
            blocktime,
            deploy_hash,
            payment_gas_limit,
            protocol_version,
            Rc::clone(&tracking_copy),
            get_payment_purse_stack,
        );

        if get_payment_result.as_error().is_some() {
            return Ok(get_payment_result);
        }

        let payment_purse = match maybe_purse {
            Some(payment_purse) => payment_purse,
            None => return Err(EngineStateError::reverter(ApiError::HandlePayment(12))),
        };

        let runtime_args = {
            let transfer_args = TransferArgs::new(
                None,
                entity.main_purse(),
                payment_purse,
                payment_amount,
                None,
            );

            match RuntimeArgs::try_from(transfer_args) {
                Ok(runtime_args) => runtime_args,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(
                        Error::CLValue(error).into(),
                    ))
                }
            }
        };

        let transfer_stack = RuntimeStack::new_system_call_stack(max_stack_height);

        let (transfer_result, payment_result) = self.invoke_mint_to_transfer(
            runtime_args,
            entity,
            entity_kind,
            authorization_keys,
            account_hash,
            blocktime,
            deploy_hash,
            payment_gas_limit,
            protocol_version,
            Rc::clone(&tracking_copy),
            transfer_stack,
            payment_amount,
        );

        if payment_result.as_error().is_some() {
            return Ok(payment_result);
        }

        let transfer_result = match transfer_result {
            Some(Ok(())) => Ok(()),
            Some(Err(mint_error)) => match mint::Error::try_from(mint_error) {
                Ok(mint_error) => Err(EngineStateError::reverter(mint_error)),
                Err(_) => Err(EngineStateError::reverter(ApiError::Transfer)),
            },
            None => Err(EngineStateError::reverter(ApiError::Transfer)),
        };

        transfer_result?;

        Ok(payment_result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_payment_purse<R>(
        &self,
        entity: &AddressableEntity,
        entity_kind: EntityKind,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        payment_gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        stack: RuntimeStack,
    ) -> (Option<URef>, ExecutionResult)
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
        R::Error: Into<Error>,
    {
        self.call_system_contract(
            DirectSystemContractCall::GetPaymentPurse,
            RuntimeArgs::new(),
            entity,
            entity_kind,
            authorization_keys,
            account_hash,
            blocktime,
            deploy_hash,
            payment_gas_limit,
            protocol_version,
            Rc::clone(&tracking_copy),
            Phase::Payment,
            stack,
            U512::zero(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn invoke_mint_to_transfer<R>(
        &self,
        runtime_args: RuntimeArgs,
        entity: &AddressableEntity,
        entity_kind: EntityKind,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        blocktime: BlockTime,
        deploy_hash: DeployHash,
        gas_limit: Gas,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        stack: RuntimeStack,
        spending_limit: U512,
    ) -> (Option<Result<(), u8>>, ExecutionResult)
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
        R::Error: Into<Error>,
    {
        self.call_system_contract(
            DirectSystemContractCall::Transfer,
            runtime_args,
            entity,
            entity_kind,
            authorization_keys,
            account_hash,
            blocktime,
            deploy_hash,
            gas_limit,
            protocol_version,
            Rc::clone(&tracking_copy),
            Phase::Payment,
            stack,
            spending_limit,
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
