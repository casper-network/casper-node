//!  This module contains all the execution related code.
pub mod deploy_item;
pub mod engine_config;
mod error;
pub mod execute_request;
pub(crate) mod execution_kind;
pub mod execution_result;

use std::{cell::RefCell, rc::Rc};

use num_traits::Zero;
use once_cell::sync::Lazy;
use tracing::error;

use casper_storage::{
    data_access_layer::TransferRequest,
    global_state::state::StateProvider,
    system::runtime_native::TransferConfig,
    tracking_copy::{FeesPurseHandling, TrackingCopyEntityExt, TrackingCopyExt},
};

use casper_types::{
    addressable_entity::EntityKind,
    system::{
        handle_payment::{self},
        HANDLE_PAYMENT,
    },
    BlockTime, DeployInfo, Digest, EntityAddr, ExecutableDeployItem, FeeHandling, Gas, Key, Motes,
    Phase, ProtocolVersion, PublicKey, RuntimeArgs, StoredValue, TransactionHash, U512,
};

use crate::{
    engine_state::{execution_kind::ExecutionKind, execution_result::ExecutionResults},
    execution::{DirectSystemContractCall, ExecError, Executor},
    runtime::RuntimeStack,
};

pub use self::{
    deploy_item::DeployItem,
    engine_config::{
        EngineConfig, EngineConfigBuilder, DEFAULT_MAX_QUERY_DEPTH,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
    },
    error::Error,
    execute_request::ExecuteRequest,
    execution_result::{ExecutionResult, ForcedTransferResult},
};

/// The maximum amount of motes that payment code execution can cost.
pub const MAX_PAYMENT_AMOUNT: u64 = 2_500_000_000;
/// The maximum amount of gas a payment code can use.
///
/// This value also indicates the minimum balance of the main purse of an account when
/// executing payment code, as such amount is held as collateral to compensate for
/// code execution.
pub static MAX_PAYMENT: Lazy<U512> = Lazy::new(|| U512::from(MAX_PAYMENT_AMOUNT));

/// Gas/motes conversion rate of wasmless transfer cost is always 1 regardless of what user wants to
/// pay.
pub const WASMLESS_TRANSFER_FIXED_GAS_PRICE: u64 = 1;

/// Main implementation of an execution engine state.
///
/// Takes an engine's configuration and a provider of a state (aka the global state) to operate on.
/// Methods implemented on this structure are the external API intended to be used by the users such
/// as the node, test framework, and others.
#[derive(Debug, Clone, Default)]
pub struct ExecutionEngineV1 {
    config: EngineConfig,
}

impl ExecutionEngineV1 {
    /// Creates new engine state.
    pub fn new(config: EngineConfig) -> ExecutionEngineV1 {
        ExecutionEngineV1 { config }
    }

    /// Returns engine config.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Runs a deploy execution request.
    ///
    /// For each deploy stored in the request it will execute it.
    ///
    /// Currently a special shortcut is taken to distinguish a native transfer, from a deploy.
    ///
    /// Return execution results which contains results from each deploy ran.
    pub fn exec(
        &self,
        state_provider: &impl StateProvider,
        mut exec_request: ExecuteRequest,
    ) -> Result<ExecutionResults, Error> {
        let deploys = exec_request.take_deploys();
        let mut results = ExecutionResults::with_capacity(deploys.len());

        for deploy_item in deploys {
            let result = match deploy_item.session {
                ExecutableDeployItem::Transfer { .. } => self.transfer(
                    state_provider,
                    exec_request.protocol_version,
                    exec_request.parent_state_hash,
                    BlockTime::new(exec_request.block_time),
                    deploy_item,
                ),
                _ => self.deploy(
                    state_provider,
                    exec_request.protocol_version,
                    exec_request.parent_state_hash,
                    BlockTime::new(exec_request.block_time),
                    deploy_item,
                    exec_request.proposer.clone(),
                ),
            };
            match result {
                Ok(result) => results.push_back(result),
                Err(error) => {
                    return Err(error);
                }
            };
        }

        Ok(results)
    }

    /// Executes a native transfer.
    ///
    /// Native transfers do not involve WASM at all, and also skip executing payment code.
    /// Therefore this is the fastest and cheapest way to transfer tokens from account to account.
    ///
    /// Returns an [`ExecutionResult`] for a successful native transfer.
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        state_provider: &impl StateProvider,
        protocol_version: ProtocolVersion,
        prestate_hash: Digest,
        blocktime: BlockTime,
        deploy_item: DeployItem,
    ) -> Result<ExecutionResult, Error> {
        // leaving this method in place for now as a lot of tests rely on it.
        // TODO: rewrite those tests to use the data access layer instead and
        // then remove this method.
        let deploy_hash = deploy_item.deploy_hash;
        let transfer_config = TransferConfig::new(
            self.config.administrative_accounts.clone(),
            self.config.allow_unrestricted_transfers,
        );

        let fee_handling = self.config.fee_handling;
        let refund_handling = self.config.refund_handling;
        let vesting_schedule_period_millis = self.config.vesting_schedule_period_millis();
        let allow_auction_bids = self.config.allow_auction_bids;
        let compute_rewards = self.config.compute_rewards;
        let max_delegators_per_validator = self.config.max_delegators_per_validator();
        let minimum_delegation_amount = self.config.minimum_delegation_amount();

        let native_runtime_config = casper_storage::system::runtime_native::Config::new(
            transfer_config,
            fee_handling,
            refund_handling,
            vesting_schedule_period_millis,
            allow_auction_bids,
            compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
        );
        let wasmless_transfer_gas = Gas::new(U512::from(
            self.config().system_config().mint_costs().transfer,
        ));

        let transaction_hash = TransactionHash::Deploy(deploy_hash);

        let transfer_req = TransferRequest::with_runtime_args(
            native_runtime_config,
            prestate_hash,
            blocktime.value(),
            protocol_version,
            transaction_hash,
            deploy_item.address,
            deploy_item.authorization_keys,
            deploy_item.session.args().clone(),
            wasmless_transfer_gas.value(),
        );
        let transfer_result = state_provider.transfer(transfer_req);
        ExecutionResult::from_transfer_result(transfer_result, wasmless_transfer_gas)
            .map_err(|_| Error::RootNotFound(prestate_hash))
    }

    /// Executes a deploy.
    ///
    /// A deploy execution consists of running the payment code, which is expected to deposit funds
    /// into the payment purse, and then running the session code with a specific gas limit. For
    /// running payment code, we lock [`MAX_PAYMENT`] amount of motes from the user as collateral.
    /// If both the payment code and the session code execute successfully, a fraction of the
    /// unspent collateral will be transferred back to the proposer of the deploy, as specified
    /// in the request.
    ///
    /// Returns [`ExecutionResult`], or an error condition.
    #[allow(clippy::too_many_arguments)]
    fn deploy(
        &self,
        state_provider: &impl StateProvider,
        protocol_version: ProtocolVersion,
        prestate_hash: Digest,
        blocktime: BlockTime,
        deploy_item: DeployItem,
        proposer: PublicKey,
    ) -> Result<ExecutionResult, Error> {
        let tracking_copy = match state_provider.tracking_copy(prestate_hash) {
            Err(gse) => return Ok(ExecutionResult::precondition_failure(Error::Storage(gse))),
            Ok(None) => return Err(Error::RootNotFound(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let executor = Executor::new(self.config().clone());

        let authorization_keys = deploy_item.authorization_keys;
        let account_hash = deploy_item.address;

        if let Err(e) = tracking_copy
            .borrow_mut()
            .migrate_account(account_hash, protocol_version)
        {
            return Ok(ExecutionResult::precondition_failure(e.into()));
        }

        // Get account from tracking copy
        // validation_spec_3: account validity
        let (entity, entity_hash) = {
            match tracking_copy
                .borrow_mut()
                .get_authorized_addressable_entity(
                    protocol_version,
                    account_hash,
                    &authorization_keys,
                    &self.config().administrative_accounts,
                ) {
                Ok((addressable_entity, entity_hash)) => (addressable_entity, entity_hash),
                Err(e) => return Ok(ExecutionResult::precondition_failure(e.into())),
            }
        };

        let entity_kind = entity.entity_kind();

        let entity_addr = EntityAddr::new_with_tag(entity_kind, entity_hash.value());

        let entity_named_keys = match tracking_copy
            .borrow_mut()
            .get_named_keys(entity_addr)
            .map_err(Into::into)
        {
            Ok(named_keys) => named_keys,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error));
            }
        };

        let payment = deploy_item.payment;
        let session = deploy_item.session;

        let deploy_hash = deploy_item.deploy_hash;

        let session_args = session.args().clone();

        // Create session code `A` from provided session bytes
        // validation_spec_1: valid wasm bytes
        // we do this upfront as there is no reason to continue if session logic is invalid
        let session_execution_kind = match ExecutionKind::new(
            Rc::clone(&tracking_copy),
            &entity_named_keys,
            session,
            &protocol_version,
            Phase::Session,
        ) {
            Ok(execution_kind) => execution_kind,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error));
            }
        };

        // Get account main purse balance key
        // validation_spec_5: account main purse minimum balance
        let entity_main_purse_key: Key = {
            let account_key = Key::URef(entity.main_purse());
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(account_key)
            {
                Ok(key) => key,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        // Get account main purse balance to enforce precondition and in case of forced
        // transfer validation_spec_5: account main purse minimum balance
        let account_main_purse_balance: Motes = match tracking_copy
            .borrow_mut()
            .get_purse_balance(entity_main_purse_key)
        {
            Ok(balance) => balance,
            Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
        };

        let max_payment_cost = Motes::new(*MAX_PAYMENT);

        // Enforce minimum main purse balance validation
        // validation_spec_5: account main purse minimum balance
        if account_main_purse_balance < max_payment_cost {
            return Ok(ExecutionResult::precondition_failure(
                Error::InsufficientPayment,
            ));
        }

        // Finalization is executed by system account (currently genesis account)
        // payment_code_spec_5: system executes finalization
        let system_addressable_entity = tracking_copy
            .borrow_mut()
            .get_addressable_entity_by_account_hash(
                protocol_version,
                PublicKey::System.to_account_hash(),
            )?;

        // Get handle payment system contract details
        // payment_code_spec_6: system contract validity
        let system_contract_registry = tracking_copy.borrow_mut().get_system_entity_registry()?;

        let handle_payment_contract_hash = system_contract_registry
            .get(HANDLE_PAYMENT)
            .ok_or_else(|| {
                error!("Missing system handle payment contract hash");
                Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
            })?;

        let handle_payment_addr =
            EntityAddr::new_system_entity_addr(handle_payment_contract_hash.value());

        let handle_payment_named_keys = match tracking_copy
            .borrow_mut()
            .get_named_keys(handle_payment_addr)
            .map_err(Into::into)
        {
            Ok(named_keys) => named_keys,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error));
            }
        };

        // Get payment purse Key from handle payment contract
        // payment_code_spec_6: system contract validity
        let payment_purse_key =
            match handle_payment_named_keys.get(handle_payment::PAYMENT_PURSE_KEY) {
                Some(key) => *key,
                None => return Ok(ExecutionResult::precondition_failure(Error::Deploy)),
            };

        let payment_purse_uref = payment_purse_key
            .into_uref()
            .ok_or(Error::InvalidKeyVariant)?;

        // [`ExecutionResultBuilder`] handles merging of multiple execution results
        let mut execution_result_builder = execution_result::ExecutionResultBuilder::new();

        let rewards_target_purse = {
            let fees_purse_handling = match self.config.fee_handling {
                FeeHandling::PayToProposer => {
                    FeesPurseHandling::ToProposer(proposer.to_account_hash())
                }
                FeeHandling::None => FeesPurseHandling::None(payment_purse_uref),
                FeeHandling::Accumulate => FeesPurseHandling::Accumulate,
                FeeHandling::Burn => FeesPurseHandling::Burn,
            };

            match tracking_copy
                .borrow_mut()
                .fees_purse(protocol_version, fees_purse_handling)
            {
                Err(tce) => return Ok(ExecutionResult::precondition_failure(tce.into())),
                Ok(purse) => purse,
            }
        };

        let rewards_target_purse_balance_key = {
            // Get reward purse Key from handle payment contract
            // payment_code_spec_6: system contract validity
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(rewards_target_purse.into())
            {
                Err(tce) => {
                    return Ok(ExecutionResult::precondition_failure(tce.into()));
                }
                Ok(key) => key,
            }
        };

        // Execute provided payment code
        let payment_result = {
            // payment_code_spec_1: init pay environment w/ gas limit == (max_payment_cost /
            // gas_price)
            let payment_gas_limit = match Gas::from_motes(max_payment_cost, deploy_item.gas_price) {
                Some(gas) => gas,
                None => {
                    return Ok(ExecutionResult::precondition_failure(
                        Error::GasConversionOverflow,
                    ))
                }
            };

            // Create payment code module from bytes
            // validation_spec_1: valid wasm bytes
            let phase = Phase::Payment;

            let payment_stack = RuntimeStack::from_account_hash(
                deploy_item.address,
                self.config.max_runtime_call_stack_height() as usize,
            );

            // payment_code_spec_2: execute payment code
            let payment_access_rights =
                entity.extract_access_rights(entity_hash, &entity_named_keys);

            let mut payment_named_keys = entity_named_keys.clone();

            let payment_args = payment.args().clone();

            if payment.is_standard_payment(phase) {
                // Todo potentially could be moved to Executor::Exec
                match executor.exec_standard_payment(
                    payment_args,
                    &entity,
                    entity_kind,
                    authorization_keys.clone(),
                    account_hash,
                    blocktime,
                    deploy_hash,
                    payment_gas_limit,
                    protocol_version,
                    Rc::clone(&tracking_copy),
                    self.config.max_runtime_call_stack_height() as usize,
                ) {
                    Ok(payment_result) => payment_result,
                    Err(error) => {
                        return Ok(ExecutionResult::precondition_failure(error));
                    }
                }
            } else {
                let payment_execution_kind = match ExecutionKind::new(
                    Rc::clone(&tracking_copy),
                    &entity_named_keys,
                    payment,
                    &protocol_version,
                    phase,
                ) {
                    Ok(execution_kind) => execution_kind,
                    Err(error) => {
                        return Ok(ExecutionResult::precondition_failure(error));
                    }
                };
                executor.exec(
                    payment_execution_kind,
                    payment_args,
                    entity_hash,
                    &entity,
                    entity_kind,
                    &mut payment_named_keys,
                    payment_access_rights,
                    authorization_keys.clone(),
                    account_hash,
                    blocktime,
                    deploy_hash,
                    payment_gas_limit,
                    protocol_version,
                    Rc::clone(&tracking_copy),
                    phase,
                    payment_stack,
                )
            }
        };
        payment_result.log_execution_result("payment result");

        // If provided wasm file was malformed, we should charge.
        if payment_result.should_charge_for_errors_in_wasm() {
            let error = payment_result
                .as_error()
                .cloned()
                .unwrap_or(Error::InsufficientPayment);

            return match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                payment_result.cost(),
                entity_main_purse_key,
                rewards_target_purse_balance_key,
            ) {
                Ok(execution_result) => Ok(execution_result),
                Err(error) => Ok(ExecutionResult::precondition_failure(error)),
            };
        }

        let payment_result_cost = payment_result.cost();
        // payment_code_spec_3: fork based upon payment purse balance and cost of
        // payment code execution

        // Get handle payment system contract details
        // payment_code_spec_6: system contract validity
        let system_contract_registry = tracking_copy.borrow_mut().get_system_entity_registry()?;

        let handle_payment_contract_hash = system_contract_registry
            .get(HANDLE_PAYMENT)
            .ok_or_else(|| {
                error!("Missing system handle payment contract hash");
                Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
            })?;

        let handle_payment_addr =
            EntityAddr::new_system_entity_addr(handle_payment_contract_hash.value());

        let handle_payment_named_keys = match tracking_copy
            .borrow_mut()
            .get_named_keys(handle_payment_addr)
            .map_err(Into::into)
        {
            Ok(named_keys) => named_keys,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error));
            }
        };

        // Get payment purse Key from handle payment contract
        // payment_code_spec_6: system contract validity
        let payment_purse_key: Key =
            match handle_payment_named_keys.get(handle_payment::PAYMENT_PURSE_KEY) {
                Some(key) => *key,
                None => return Ok(ExecutionResult::precondition_failure(Error::Deploy)),
            };
        let purse_balance_key = match tracking_copy
            .borrow_mut()
            .get_purse_balance_key(payment_purse_key)
        {
            Ok(key) => key,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };
        let payment_purse_balance: Motes = {
            match tracking_copy
                .borrow_mut()
                .get_purse_balance(purse_balance_key)
            {
                Ok(balance) => balance,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        if let Some(forced_transfer) =
            payment_result.check_forced_transfer(payment_purse_balance, deploy_item.gas_price)
        {
            // Get rewards purse balance key
            // payment_code_spec_6: system contract validity
            let error = match forced_transfer {
                ForcedTransferResult::InsufficientPayment => Error::InsufficientPayment,
                ForcedTransferResult::GasConversionOverflow => Error::GasConversionOverflow,
                ForcedTransferResult::PaymentFailure => payment_result
                    .take_error()
                    .unwrap_or(Error::InsufficientPayment),
            };

            let gas_cost = match Gas::from_motes(max_payment_cost, deploy_item.gas_price) {
                Some(gas) => gas,
                None => {
                    return Ok(ExecutionResult::precondition_failure(
                        Error::GasConversionOverflow,
                    ))
                }
            };

            return match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                gas_cost,
                entity_main_purse_key,
                rewards_target_purse_balance_key,
            ) {
                Ok(execution_result) => Ok(execution_result),
                Err(error) => Ok(ExecutionResult::precondition_failure(error)),
            };
        };

        // Transfer the contents of the rewards purse to block proposer
        execution_result_builder.set_payment_execution_result(payment_result);

        // Begin session logic handling
        let post_payment_tracking_copy = tracking_copy.borrow();
        let session_tracking_copy = Rc::new(RefCell::new(post_payment_tracking_copy.fork()));

        let session_stack = RuntimeStack::from_account_hash(
            deploy_item.address,
            self.config.max_runtime_call_stack_height() as usize,
        );

        let mut session_named_keys = entity_named_keys.clone();

        let session_access_rights = entity.extract_access_rights(entity_hash, &entity_named_keys);

        let mut session_result = {
            // payment_code_spec_3_b_i: if (balance of handle payment pay purse) >= (gas spent
            // during payment code execution) * gas_price, yes session
            // session_code_spec_1: gas limit = ((balance of handle payment payment purse) /
            // gas_price)
            // - (gas spent during payment execution)
            let session_gas_limit: Gas =
                match Gas::from_motes(payment_purse_balance, deploy_item.gas_price)
                    .and_then(|gas| gas.checked_sub(payment_result_cost))
                {
                    Some(gas) => gas,
                    None => {
                        return Ok(ExecutionResult::precondition_failure(
                            Error::GasConversionOverflow,
                        ))
                    }
                };

            executor.exec(
                session_execution_kind,
                session_args,
                entity_hash,
                &entity,
                entity_kind,
                &mut session_named_keys,
                session_access_rights,
                authorization_keys.clone(),
                account_hash,
                blocktime,
                deploy_hash,
                session_gas_limit,
                protocol_version,
                Rc::clone(&session_tracking_copy),
                Phase::Session,
                session_stack,
            )
        };
        session_result.log_execution_result("session result");

        // Create + persist deploy info.
        {
            let transfers = session_result.transfers();
            let cost = payment_result_cost.value() + session_result.cost().value();
            let deploy_info = DeployInfo::new(
                deploy_hash,
                transfers,
                account_hash,
                entity.main_purse(),
                cost,
            );
            session_tracking_copy.borrow_mut().write(
                Key::DeployInfo(deploy_hash),
                StoredValue::DeployInfo(deploy_info),
            );
        }

        // Session execution was zero cost or provided wasm was malformed.
        // Check if the payment purse can cover the minimum floor for session execution.
        if (session_result.cost().is_zero() && payment_purse_balance < max_payment_cost)
            || session_result.should_charge_for_errors_in_wasm()
        {
            // When session code structure is valid but still has 0 cost we should propagate the
            // error.
            let error = session_result
                .as_error()
                .cloned()
                .unwrap_or(Error::InsufficientPayment);

            return match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                session_result.cost(),
                entity_main_purse_key,
                rewards_target_purse_balance_key,
            ) {
                Ok(execution_result) => Ok(execution_result),
                Err(error) => Ok(ExecutionResult::precondition_failure(error)),
            };
        }

        let post_session_rc = if session_result.is_failure() {
            // If session code fails we do not include its effects,
            // so we start again from the post-payment state.
            Rc::new(RefCell::new(post_payment_tracking_copy.fork()))
        } else {
            session_result = session_result.with_effects(session_tracking_copy.borrow().effects());
            session_tracking_copy
        };

        // NOTE: session_code_spec_3: (do not include session execution effects in
        // results) is enforced in execution_result_builder.build()
        execution_result_builder.set_session_execution_result(session_result);

        // payment_code_spec_5: run finalize process
        let finalize_result: ExecutionResult = {
            let post_session_tc = post_session_rc.borrow();
            let finalization_tc = Rc::new(RefCell::new(post_session_tc.fork()));

            let handle_payment_args = {
                //((gas spent during payment code execution) + (gas spent during session code execution)) * gas_price
                let finalize_cost_motes = match Motes::from_gas(
                    execution_result_builder.total_cost(),
                    deploy_item.gas_price,
                ) {
                    Some(motes) => motes,
                    None => {
                        return Ok(ExecutionResult::precondition_failure(
                            Error::GasConversionOverflow,
                        ))
                    }
                };

                let maybe_runtime_args = RuntimeArgs::try_new(|args| {
                    args.insert(handle_payment::ARG_AMOUNT, finalize_cost_motes.value())?;
                    args.insert(handle_payment::ARG_ACCOUNT, account_hash)?;
                    args.insert(handle_payment::ARG_TARGET, rewards_target_purse)?;
                    Ok(())
                });
                match maybe_runtime_args {
                    Ok(runtime_args) => runtime_args,
                    Err(error) => {
                        let exec_error = ExecError::from(error);
                        return Ok(ExecutionResult::precondition_failure(exec_error.into()));
                    }
                }
            };

            // The Handle Payment keys may have changed because of effects during payment and/or
            // session, so we need to look them up again from the tracking copy
            let system_contract_registry =
                finalization_tc.borrow_mut().get_system_entity_registry()?;

            let handle_payment_contract_hash = system_contract_registry
                .get(HANDLE_PAYMENT)
                .ok_or_else(|| {
                    error!("Missing system handle payment contract hash");
                    Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
                })?;

            let handle_payment_contract = match finalization_tc
                .borrow_mut()
                .get_addressable_entity_by_hash(*handle_payment_contract_hash)
            {
                Ok(info) => info,
                Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
            };

            let handle_payment_addr =
                EntityAddr::new_system_entity_addr(handle_payment_contract_hash.value());

            let handle_payment_named_keys = match finalization_tc
                .borrow_mut()
                .get_named_keys(handle_payment_addr)
            {
                Ok(named_keys) => named_keys,
                Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
            };

            let mut handle_payment_access_rights = handle_payment_contract
                .extract_access_rights(*handle_payment_contract_hash, &handle_payment_named_keys);
            handle_payment_access_rights.extend(&[payment_purse_uref, rewards_target_purse]);

            let gas_limit = Gas::new(U512::MAX);

            let handle_payment_stack = RuntimeStack::new_system_call_stack(
                self.config.max_runtime_call_stack_height() as usize,
            );
            let system_account_hash = PublicKey::System.to_account_hash();

            let (_ret, finalize_result): (Option<()>, ExecutionResult) = executor
                .call_system_contract(
                    DirectSystemContractCall::FinalizePayment,
                    handle_payment_args,
                    &system_addressable_entity,
                    EntityKind::Account(system_account_hash),
                    authorization_keys,
                    system_account_hash,
                    blocktime,
                    deploy_hash,
                    gas_limit,
                    protocol_version,
                    finalization_tc,
                    Phase::FinalizePayment,
                    handle_payment_stack,
                    U512::zero(),
                );

            finalize_result
        };

        execution_result_builder.set_finalize_execution_result(finalize_result);

        // We panic here to indicate that the builder was not used properly.
        let ret = execution_result_builder
            .build()
            .expect("ExecutionResultBuilder not initialized properly");

        // NOTE: payment_code_spec_5_a is enforced in execution_result_builder.build()
        // payment_code_spec_6: return properly combined set of transforms and
        // appropriate error
        Ok(ret)
    }
}
