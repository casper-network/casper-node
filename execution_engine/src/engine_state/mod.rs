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
use tracing::{debug, error, trace, warn};

use casper_storage::{
    data_access_layer::{
        balance::BalanceResult,
        bids::{BidsRequest, BidsResult},
        block_rewards::{BlockRewardsRequest, BlockRewardsResult},
        prune::{PruneRequest, PruneResult},
        query::{QueryRequest, QueryResult},
        step::StepRequest,
        DataAccessLayer, EraValidatorsRequest, EraValidatorsResult, FeeRequest, FeeResult,
        GenesisRequest, GenesisResult, ProtocolUpgradeRequest, ProtocolUpgradeResult, StepResult,
        TransferRequest, TrieRequest,
    },
    global_state::{
        self,
        error::Error as GlobalStateError,
        state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, StateProvider,
        },
        trie::TrieRaw,
    },
    system::{
        auction,
        runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
    },
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};

use casper_types::{
    account::AccountHash,
    addressable_entity::{EntityKind, EntityKindTag, NamedKeys},
    execution::Effects,
    system::{
        auction::SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        handle_payment::{self, ACCUMULATION_PURSE_KEY},
        AUCTION, HANDLE_PAYMENT, MINT,
    },
    AddressableEntity, AddressableEntityHash, BlockTime, DeployInfo, Digest, EntityAddr,
    ExecutableDeployItem, FeeHandling, Gas, Key, KeyTag, Motes, Phase, ProtocolVersion, PublicKey,
    RuntimeArgs, StoredValue, SystemEntityRegistry, TransactionHash, URef, U512,
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
#[derive(Debug)]
pub struct EngineState<S> {
    config: EngineConfig,
    state: S,
}

impl EngineState<ScratchGlobalState> {
    /// Returns the inner state
    pub fn into_inner(self) -> ScratchGlobalState {
        self.state
    }
}

impl EngineState<DataAccessLayer<LmdbGlobalState>> {
    /// Provide a local cached-only version of engine-state.
    pub fn get_scratch_engine_state(&self) -> EngineState<ScratchGlobalState> {
        EngineState {
            config: self.config.clone(),
            state: self.state.state().create_scratch(),
        }
    }

    /// Gets underlying LmdbGlobalState
    pub fn get_state(&self) -> &DataAccessLayer<LmdbGlobalState> {
        &self.state
    }

    /// Flushes the LMDB environment to disk when manual sync is enabled in the config.toml.
    pub fn flush_environment(&self) -> Result<(), global_state::error::Error> {
        self.state.flush_environment()
    }

    /// Writes state cached in an `EngineState<ScratchEngineState>` to LMDB.
    pub fn write_scratch_to_db(
        &self,
        state_root_hash: Digest,
        scratch_global_state: ScratchGlobalState,
    ) -> Result<Digest, global_state::error::Error> {
        self.state
            .write_scratch_to_db(state_root_hash, scratch_global_state)
    }
}

impl<S> EngineState<S>
where
    S: StateProvider + CommitProvider,
{
    /// Creates new engine state.
    pub fn new(state: S, config: EngineConfig) -> EngineState<S> {
        EngineState { config, state }
    }

    /// Returns engine config.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Updates current engine config with a new instance.
    pub fn update_config(&mut self, new_config: EngineConfig) {
        self.config = new_config
    }

    /// Creates a new tracking copy instance.
    pub fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<S::Reader>>, GlobalStateError> {
        self.state.tracking_copy(hash)
    }

    /// Commits genesis process.
    ///
    /// This process is run only once per network to initiate the system. By definition users are
    /// unable to execute smart contracts on a network without a genesis.
    ///
    /// Takes genesis configuration passed through [`GenesisRequest`] and creates the system
    /// contracts, sets up the genesis accounts, and sets up the auction state based on that. At
    /// the end of the process, [`SystemEntityRegistry`] is persisted under the special global
    /// state space [`Key::SystemEntityRegistry`].
    pub fn commit_genesis(&self, request: GenesisRequest) -> GenesisResult {
        self.state.genesis(request)
    }

    /// Commits upgrade.
    ///
    /// This process applies changes to the global state.
    ///
    /// Returns [`ProtocolUpgradeResult`].
    pub fn commit_upgrade(&self, request: ProtocolUpgradeRequest) -> ProtocolUpgradeResult {
        self.state.protocol_upgrade(request)
    }

    /// Commit a prune of leaf nodes from the tip of the merkle trie.
    pub fn commit_prune(&self, request: PruneRequest) -> PruneResult {
        self.state.prune(request)
    }

    /// Executes a step request.
    pub fn commit_step(&self, request: StepRequest) -> StepResult {
        self.state.step(request)
    }

    /// Distribute block rewards.
    pub fn commit_fees(&self, request: FeeRequest) -> FeeResult {
        self.state.distribute_fees(request)
    }

    /// Distribute block rewards.
    pub fn commit_block_rewards(&self, request: BlockRewardsRequest) -> BlockRewardsResult {
        self.state.distribute_block_rewards(request)
    }

    /// Commit effects of the execution.
    ///
    /// This method has to be run after an execution has been made to persists the effects of it.
    ///
    /// Returns new state root hash.
    pub fn commit_effects(
        &self,
        pre_state_hash: Digest,
        effects: Effects,
    ) -> Result<Digest, Error> {
        self.state
            .commit(pre_state_hash, effects)
            .map_err(|err| Error::Exec(err.into()))
    }

    /// Executes a query.
    ///
    /// For a given root [`Key`] it does a path lookup through the named keys.
    ///
    /// Returns the value stored under a [`URef`] wrapped in a [`QueryResult`].
    pub fn run_query(&self, query_request: QueryRequest) -> QueryResult {
        self.state.query(query_request)
    }

    /// Runs a deploy execution request.
    ///
    /// For each deploy stored in the request it will execute it.
    ///
    /// Currently a special shortcut is taken to distinguish a native transfer, from a deploy.
    ///
    /// Return execution results which contains results from each deploy ran.
    pub fn run_execute(&self, mut exec_request: ExecuteRequest) -> Result<ExecutionResults, Error> {
        let executor = Executor::new(self.config().clone());

        let deploys = exec_request.take_deploys();
        let mut results = ExecutionResults::with_capacity(deploys.len());

        for deploy_item in deploys {
            let result = match deploy_item.session {
                ExecutableDeployItem::Transfer { .. } => self.transfer(
                    &executor,
                    exec_request.protocol_version,
                    exec_request.parent_state_hash,
                    BlockTime::new(exec_request.block_time),
                    deploy_item,
                ),
                _ => self.deploy(
                    &executor,
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

    fn get_named_keys(
        &self,
        entity_addr: EntityAddr,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Result<NamedKeys, Error> {
        tracking_copy
            .borrow_mut()
            .get_named_keys(entity_addr)
            .map_err(Into::into)
    }

    /// Get the balance of a passed purse referenced by its [`URef`].
    pub fn get_purse_balance(
        &self,
        state_hash: Digest,
        purse_uref: URef,
    ) -> Result<BalanceResult, Error> {
        let tracking_copy = match self.tracking_copy(state_hash)? {
            Some(tracking_copy) => tracking_copy,
            None => return Ok(BalanceResult::RootNotFound),
        };
        let purse_balance_key = tracking_copy.get_purse_balance_key(purse_uref.into())?;
        let (balance, proof) = tracking_copy.get_purse_balance_with_proof(purse_balance_key)?;
        let proof = Box::new(proof);
        let motes = balance.value();
        Ok(BalanceResult::Success { motes, proof })
    }

    /// Executes a native transfer.
    ///
    /// Native transfers do not involve WASM at all, and also skip executing payment code.
    /// Therefore this is the fastest and cheapest way to transfer tokens from account to account.
    ///
    /// Returns an [`ExecutionResult`] for a successful native transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer(
        &self,
        _executor: &Executor,
        protocol_version: ProtocolVersion,
        prestate_hash: Digest,
        blocktime: BlockTime,
        deploy_item: DeployItem,
    ) -> Result<ExecutionResult, Error> {
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

        let native_runtime_config = NativeRuntimeConfig::new(
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
        let transfer_result = self.state.transfer(transfer_req);
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
    pub fn deploy(
        &self,
        executor: &Executor,
        protocol_version: ProtocolVersion,
        prestate_hash: Digest,
        blocktime: BlockTime,
        deploy_item: DeployItem,
        proposer: PublicKey,
    ) -> Result<ExecutionResult, Error> {
        // spec: https://casperlabs.atlassian.net/wiki/spaces/EN/pages/123404576/Payment+code+execution+specification

        // Create tracking copy (which functions as a deploy context)
        // validation_spec_2: prestate_hash check
        // do this second; as there is no reason to proceed if the prestate hash is invalid
        let tracking_copy = match self.tracking_copy(prestate_hash) {
            Err(gse) => return Ok(ExecutionResult::precondition_failure(Error::Storage(gse))),
            Ok(None) => return Err(Error::RootNotFound(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        // Get addr bytes from `address` (which is actually a Key)
        // validation_spec_3: account validity

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

        let entity_named_keys = match self.get_named_keys(entity_addr, Rc::clone(&tracking_copy)) {
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
            .read_addressable_entity_by_account_hash(
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

        let handle_payment_named_keys =
            match self.get_named_keys(handle_payment_addr, Rc::clone(&tracking_copy)) {
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

        let rewards_target_purse =
            match self.get_rewards_purse(protocol_version, proposer, prestate_hash) {
                Ok(target_purse) => target_purse,
                Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            };

        let rewards_target_purse_balance_key = {
            // Get reward purse Key from handle payment contract
            // payment_code_spec_6: system contract validity
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(rewards_target_purse.into())
            {
                Ok(key) => key,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
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
        log_execution_result("payment result", &payment_result);

        // If provided wasm file was malformed, we should charge.
        if should_charge_for_errors_in_wasm(&payment_result) {
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

        let handle_payment_named_keys =
            match self.get_named_keys(handle_payment_addr, Rc::clone(&tracking_copy)) {
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
        log_execution_result("session result", &session_result);

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
            || should_charge_for_errors_in_wasm(&session_result)
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
                .get_addressable_entity(*handle_payment_contract_hash)
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

            let handle_payment_stack = self.get_new_system_call_stack();
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

    fn get_rewards_purse(
        &self,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
        prestate_hash: Digest,
    ) -> Result<URef, Error> {
        let tracking_copy = match self.tracking_copy(prestate_hash) {
            Err(gse) => return Err(Error::Storage(gse)),
            Ok(None) => return Err(Error::RootNotFound(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };
        match self.config.fee_handling() {
            FeeHandling::PayToProposer => {
                // the proposer of the block this deploy is in receives the gas from this deploy
                // execution
                let proposer_account: AddressableEntity = match tracking_copy
                    .borrow_mut()
                    .get_addressable_entity_by_account_hash(
                        protocol_version,
                        AccountHash::from(&proposer),
                    ) {
                    Ok(account) => account,
                    Err(error) => return Err(error.into()),
                };

                Ok(proposer_account.main_purse())
            }
            FeeHandling::Accumulate => {
                let handle_payment_hash = self.get_handle_payment_hash(prestate_hash)?;

                let handle_payment_named_keys = tracking_copy
                    .borrow_mut()
                    .get_named_keys(EntityAddr::System(handle_payment_hash.value()))?;

                let accumulation_purse_uref =
                    match handle_payment_named_keys.get(ACCUMULATION_PURSE_KEY) {
                        Some(Key::URef(accumulation_purse)) => accumulation_purse,
                        Some(_) | None => {
                            error!(
                            "fee handling is configured to accumulate but handle payment does not \
                            have accumulation purse"
                        );
                            return Err(Error::FailedToRetrieveAccumulationPurse);
                        }
                    };

                Ok(*accumulation_purse_uref)
            }
            FeeHandling::Burn => Ok(URef::default()),
        }
    }

    /// Gets a trie object for given state root hash.
    pub fn get_trie_full(&self, trie_key: Digest) -> Result<Option<TrieRaw>, Error> {
        let req = TrieRequest::new(trie_key, None);
        self.state.trie(req).into_legacy().map_err(Error::Storage)
    }

    /// Obtains validator weights for given era.
    ///
    /// This skips execution of auction's `get_era_validator` entry point logic to avoid creating an
    /// executor instance, and going through the execution flow. It follows the same process but
    /// uses queries rather than execution to get the snapshot.
    pub fn get_era_validators(
        &self,
        get_era_validators_request: EraValidatorsRequest,
    ) -> EraValidatorsResult {
        let state_root_hash = get_era_validators_request.state_hash();

        let system_contract_registry = match self.get_system_contract_registry(state_root_hash) {
            Ok(system_contract_registry) => system_contract_registry,
            Err(error) => {
                error!(%state_root_hash, %error, "auction not found");
                return EraValidatorsResult::AuctionNotFound;
            }
        };

        let query_request = match system_contract_registry.get(AUCTION).copied() {
            Some(auction_hash) => QueryRequest::new(
                state_root_hash,
                Key::addressable_entity_key(EntityKindTag::System, auction_hash),
                vec![SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.to_string()],
            ),
            None => return EraValidatorsResult::AuctionNotFound,
        };

        let snapshot = match self.run_query(query_request) {
            QueryResult::RootNotFound => return EraValidatorsResult::RootNotFound,
            QueryResult::Failure(error) => {
                error!(%error, "unexpected tracking copy error");
                return EraValidatorsResult::Failure(error);
            }
            QueryResult::ValueNotFound(message) => {
                error!(%message, "value not found");
                return EraValidatorsResult::ValueNotFound(message);
            }
            QueryResult::Success { value, proofs: _ } => {
                let cl_value = match value.into_cl_value() {
                    Some(snapshot_cl_value) => snapshot_cl_value,
                    None => {
                        error!("unexpected query failure; seigniorage recipients snapshot is not a CLValue");
                        return EraValidatorsResult::Failure(
                            TrackingCopyError::UnexpectedStoredValueVariant,
                        );
                    }
                };

                match cl_value.into_t() {
                    Ok(snapshot) => snapshot,
                    Err(cve) => {
                        return EraValidatorsResult::Failure(TrackingCopyError::CLValue(cve));
                    }
                }
            }
        };
        let era_validators = auction::detail::era_validators_from_snapshot(snapshot);
        EraValidatorsResult::Success { era_validators }
    }

    /// Gets current bids from the auction system.
    pub fn get_bids(&self, get_bids_request: BidsRequest) -> BidsResult {
        let state_root_hash = get_bids_request.state_hash();
        let tracking_copy = match self.state.checkout(state_root_hash) {
            Ok(ret) => match ret {
                Some(tracking_copy) => Rc::new(RefCell::new(TrackingCopy::new(
                    tracking_copy,
                    self.config.max_query_depth,
                ))),
                None => return BidsResult::RootNotFound,
            },
            Err(err) => return BidsResult::Failure(TrackingCopyError::Storage(err)),
        };

        let mut tc = tracking_copy.borrow_mut();

        let bid_keys = match tc.get_keys(&KeyTag::BidAddr) {
            Ok(ret) => ret,
            Err(err) => return BidsResult::Failure(err),
        };

        let mut bids = vec![];
        for key in bid_keys.iter() {
            match tc.get(key) {
                Ok(ret) => match ret {
                    Some(StoredValue::BidKind(bid_kind)) => {
                        bids.push(bid_kind);
                    }
                    Some(_) => {
                        return BidsResult::Failure(TrackingCopyError::UnexpectedStoredValueVariant)
                    }
                    None => return BidsResult::Failure(TrackingCopyError::MissingBid(*key)),
                },
                Err(error) => return BidsResult::Failure(error),
            }
        }
        BidsResult::Success { bids }
    }

    /// Gets the balance of a given public key.
    pub fn get_balance(
        &self,
        state_hash: Digest,
        public_key: PublicKey,
    ) -> Result<BalanceResult, Error> {
        // Look up the account, get the main purse, and then do the existing balance check
        let tracking_copy = match self.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return Ok(BalanceResult::RootNotFound),
            Err(error) => return Err(error.into()),
        };

        let account_addr = public_key.to_account_hash();

        let entity_hash = match tracking_copy
            .borrow_mut()
            .get_entity_hash_by_account_hash(account_addr)
        {
            Ok(account) => account,
            Err(error) => return Err(error.into()),
        };

        let account = match tracking_copy
            .borrow_mut()
            .read(&Key::addressable_entity_key(
                EntityKindTag::Account,
                entity_hash,
            ))
            .map_err(|_| Error::InvalidKeyVariant)?
        {
            Some(StoredValue::AddressableEntity(account)) => account,
            Some(_) | None => return Err(Error::InvalidKeyVariant),
        };

        let main_purse_balance_key = {
            let main_purse = account.main_purse();
            match tracking_copy
                .borrow()
                .get_purse_balance_key(main_purse.into())
            {
                Ok(balance_key) => balance_key,
                Err(error) => return Err(error.into()),
            }
        };

        let (account_balance, proof) = match tracking_copy
            .borrow()
            .get_purse_balance_with_proof(main_purse_balance_key)
        {
            Ok((balance, proof)) => (balance, proof),
            Err(error) => return Err(error.into()),
        };

        let proof = Box::new(proof);
        let motes = account_balance.value();
        Ok(BalanceResult::Success { motes, proof })
    }

    /// Obtains an instance of a system contract registry for a given state root hash.
    pub fn get_system_contract_registry(
        &self,
        state_root_hash: Digest,
    ) -> Result<SystemEntityRegistry, Error> {
        let tracking_copy = match self.tracking_copy(state_root_hash)? {
            None => return Err(Error::RootNotFound(state_root_hash)),
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
        };
        let result = tracking_copy
            .borrow_mut()
            .get_system_entity_registry()
            .map_err(|error| {
                warn!(%error, "Failed to retrieve system contract registry");
                Error::MissingSystemContractRegistry
            });
        result
    }

    /// Returns mint system contract hash.
    pub fn get_system_mint_hash(&self, state_hash: Digest) -> Result<AddressableEntityHash, Error> {
        let registry = self.get_system_contract_registry(state_hash)?;
        let mint_hash = registry.get(MINT).ok_or_else(|| {
            error!("Missing system mint contract hash");
            Error::MissingSystemContractHash(MINT.to_string())
        })?;
        Ok(*mint_hash)
    }

    /// Returns auction system contract hash.
    pub fn get_system_auction_hash(
        &self,
        state_hash: Digest,
    ) -> Result<AddressableEntityHash, Error> {
        let registry = self.get_system_contract_registry(state_hash)?;
        let auction_hash = registry.get(AUCTION).ok_or_else(|| {
            error!("Missing system auction contract hash");
            Error::MissingSystemContractHash(AUCTION.to_string())
        })?;
        Ok(*auction_hash)
    }

    /// Returns handle payment system contract hash.
    pub fn get_handle_payment_hash(
        &self,
        state_hash: Digest,
    ) -> Result<AddressableEntityHash, Error> {
        let registry = self.get_system_contract_registry(state_hash)?;
        let handle_payment = registry.get(HANDLE_PAYMENT).ok_or_else(|| {
            error!("Missing system handle payment contract hash");
            Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
        })?;
        Ok(*handle_payment)
    }

    fn get_new_system_call_stack(&self) -> RuntimeStack {
        let max_height = self.config.max_runtime_call_stack_height() as usize;
        RuntimeStack::new_system_call_stack(max_height)
    }
}

fn log_execution_result(preamble: &'static str, result: &ExecutionResult) {
    trace!("{}: {:?}", preamble, result);
    match result {
        ExecutionResult::Success {
            transfers,
            cost,
            effects,
            messages,
        } => {
            debug!(
                %cost,
                transfer_count = %transfers.len(),
                transforms_count = %effects.len(),
                messages_count = %messages.len(),
                "{}: execution success",
                preamble
            );
        }
        ExecutionResult::Failure {
            error,
            transfers,
            cost,
            effects,
            messages,
        } => {
            debug!(
                %error,
                %cost,
                transfer_count = %transfers.len(),
                transforms_count = %effects.len(),
                messages_count = %messages.len(),
                "{}: execution failure",
                preamble
            );
        }
    }
}

fn should_charge_for_errors_in_wasm(execution_result: &ExecutionResult) -> bool {
    match execution_result {
        ExecutionResult::Failure {
            error,
            transfers: _,
            cost: _,
            effects: _,
            messages: _,
        } => match error {
            Error::Exec(err) => match err {
                ExecError::WasmPreprocessing(_) | ExecError::UnsupportedWasmStart => true,
                ExecError::Storage(_)
                | ExecError::InvalidByteCode(_)
                | ExecError::WasmOptimizer
                | ExecError::ParityWasm(_)
                | ExecError::Interpreter(_)
                | ExecError::BytesRepr(_)
                | ExecError::NamedKeyNotFound(_)
                | ExecError::KeyNotFound(_)
                | ExecError::AccountNotFound(_)
                | ExecError::TypeMismatch(_)
                | ExecError::InvalidAccess { .. }
                | ExecError::ForgedReference(_)
                | ExecError::URefNotFound(_)
                | ExecError::FunctionNotFound(_)
                | ExecError::GasLimit
                | ExecError::Ret(_)
                | ExecError::Resolver(_)
                | ExecError::Revert(_)
                | ExecError::AddKeyFailure(_)
                | ExecError::RemoveKeyFailure(_)
                | ExecError::UpdateKeyFailure(_)
                | ExecError::SetThresholdFailure(_)
                | ExecError::SystemContract(_)
                | ExecError::DeploymentAuthorizationFailure
                | ExecError::UpgradeAuthorizationFailure
                | ExecError::ExpectedReturnValue
                | ExecError::UnexpectedReturnValue
                | ExecError::InvalidContext
                | ExecError::IncompatibleProtocolMajorVersion { .. }
                | ExecError::CLValue(_)
                | ExecError::HostBufferEmpty
                | ExecError::NoActiveEntityVersions(_)
                | ExecError::InvalidEntityVersion(_)
                | ExecError::NoSuchMethod(_)
                | ExecError::TemplateMethod(_)
                | ExecError::KeyIsNotAURef(_)
                | ExecError::UnexpectedStoredValueVariant
                | ExecError::LockedEntity(_)
                | ExecError::InvalidPackage(_)
                | ExecError::InvalidEntity(_)
                | ExecError::MissingArgument { .. }
                | ExecError::DictionaryItemKeyExceedsLength
                | ExecError::MissingSystemContractRegistry
                | ExecError::MissingSystemContractHash(_)
                | ExecError::RuntimeStackOverflow
                | ExecError::ValueTooLarge
                | ExecError::MissingRuntimeStack
                | ExecError::DisabledEntity(_)
                | ExecError::UnexpectedKeyVariant(_)
                | ExecError::InvalidEntityKind(_)
                | ExecError::TrackingCopy(_)
                | ExecError::Transform(_)
                | ExecError::InvalidEntryPointType
                | ExecError::InvalidMessageTopicOperation
                | ExecError::InvalidUtf8Encoding(_) => false,
                ExecError::DisabledUnrestrictedTransfers => false,
            },
            Error::WasmPreprocessing(_) => true,
            Error::WasmSerialization(_) => true,
            Error::RootNotFound(_)
            | Error::InvalidProtocolVersion(_)
            | Error::Genesis(_)
            | Error::Storage(_)
            | Error::Authorization
            | Error::MissingContractByAccountHash(_)
            | Error::MissingEntityPackage(_)
            | Error::InsufficientPayment
            | Error::GasConversionOverflow
            | Error::Deploy
            | Error::Finalization
            | Error::Bytesrepr(_)
            | Error::Mint(_)
            | Error::InvalidKeyVariant
            | Error::ProtocolUpgrade(_)
            | Error::InvalidDeployItemVariant(_)
            | Error::CommitError(_)
            | Error::MissingSystemContractRegistry
            | Error::MissingSystemContractHash(_)
            | Error::MissingChecksumRegistry
            | Error::RuntimeStackOverflow
            | Error::FailedToGetKeys(_)
            | Error::FailedToGetStoredWithdraws
            | Error::FailedToGetWithdrawPurses
            | Error::FailedToRetrieveUnbondingDelay
            | Error::FailedToRetrieveEraId
            | Error::MissingTrieNodeChildren(_)
            | Error::FailedToRetrieveAccumulationPurse
            | Error::FailedToPrune(_)
            | Error::Transfer(_)
            | Error::TrackingCopy(_) => false,
        },
        ExecutionResult::Success { .. } => false,
    }
}
