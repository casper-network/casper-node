//!  This module contains all the execution related code.
pub mod balance;
pub mod chainspec_registry;
pub mod deploy_item;
pub mod engine_config;
pub mod era_validators;
mod error;
pub mod executable_deploy_item;
pub mod execute_request;
pub mod execution_effect;
pub mod execution_result;
pub mod genesis;
pub mod get_bids;
pub mod op;
pub mod query;
pub mod run_genesis_request;
pub mod step;
pub mod system_contract_registry;
mod transfer;
pub mod upgrade;

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    rc::Rc,
};

use num::Zero;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use tracing::{debug, error};

use casper_hashing::Digest;
use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::{Bytes, ToBytes},
    contracts::NamedKeys,
    system::{
        auction::{
            EraValidators, ARG_ERA_END_TIMESTAMP_MILLIS, ARG_EVICTED_VALIDATORS, ARG_VALIDATOR,
            ARG_VALIDATOR_PUBLIC_KEYS, AUCTION_DELAY_KEY, LOCKED_FUNDS_PERIOD_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment,
        mint::{self, ROUND_SEIGNIORAGE_RATE_KEY},
        AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AccessRights, ApiError, BlockTime, CLValue, ContractHash, DeployHash, DeployInfo, Gas, Key,
    KeyTag, Motes, Phase, ProtocolVersion, PublicKey, RuntimeArgs, StoredValue, URef, U512,
};

pub use self::{
    balance::{BalanceRequest, BalanceResult},
    chainspec_registry::ChainspecRegistry,
    deploy_item::DeployItem,
    engine_config::{EngineConfig, DEFAULT_MAX_QUERY_DEPTH, DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT},
    era_validators::{GetEraValidatorsError, GetEraValidatorsRequest},
    error::Error,
    executable_deploy_item::{ExecutableDeployItem, ExecutableDeployItemIdentifier},
    execute_request::ExecuteRequest,
    execution::Error as ExecError,
    execution_result::{ExecutionResult, ForcedTransferResult},
    genesis::{ExecConfig, GenesisAccount, GenesisConfig, GenesisSuccess},
    get_bids::{GetBidsRequest, GetBidsResult},
    query::{QueryRequest, QueryResult},
    run_genesis_request::RunGenesisRequest,
    step::{SlashItem, StepError, StepRequest, StepSuccess},
    system_contract_registry::SystemContractRegistry,
    transfer::{TransferArgs, TransferRuntimeArgsBuilder, TransferTargetMode},
    upgrade::{UpgradeConfig, UpgradeSuccess},
};
use crate::{
    core::{
        engine_state::{
            executable_deploy_item::ExecutionKind,
            execution_result::{ExecutionResultBuilder, ExecutionResults},
            genesis::GenesisInstaller,
            upgrade::{ProtocolUpgradeError, SystemUpgrader},
        },
        execution::{self, DirectSystemContractCall, Executor},
        runtime::RuntimeStack,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::{
        global_state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, StateProvider,
        },
        trie::{TrieOrChunk, TrieOrChunkId},
    },
    system::auction,
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

impl EngineState<LmdbGlobalState> {
    /// Gets underlyng LmdbGlobalState
    pub fn get_state(&self) -> &LmdbGlobalState {
        &self.state
    }

    /// Flushes the LMDB environment to disk when manual sync is enabled in the config.toml.
    pub fn flush_environment(&self) -> Result<(), lmdb::Error> {
        if self.state.environment.is_manual_sync_enabled() {
            self.state.environment.sync()?
        }
        Ok(())
    }

    /// Provide a local cached-only version of engine-state.
    pub fn get_scratch_engine_state(&self) -> EngineState<ScratchGlobalState> {
        EngineState {
            config: self.config,
            state: self.state.create_scratch(),
        }
    }

    /// Writes state cached in an EngineState<ScratchEngineState> to LMDB.
    pub fn write_scratch_to_db(
        &self,
        state_root_hash: Digest,
        scratch_global_state: ScratchGlobalState,
    ) -> Result<Digest, Error> {
        let stored_values = scratch_global_state.into_inner();
        self.state
            .put_stored_values(CorrelationId::new(), state_root_hash, stored_values)
            .map_err(Into::into)
    }
}

impl<S> EngineState<S>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
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

    /// Commits genesis process.
    ///
    /// This process is run only once per network to initiate the system. By definition users are
    /// unable to execute smart contracts on a network without a genesis.
    ///
    /// Takes genesis configuration passed through [`ExecConfig`] and creates the system contracts,
    /// sets up the genesis accounts, and sets up the auction state based on that. At the end of
    /// the process, [`SystemContractRegistry`] is persisted under the special global state space
    /// [`Key::SystemContractRegistry`].
    ///
    /// Returns a [`GenesisSuccess`] for a successful operation, or an error otherwise.
    pub fn commit_genesis(
        &self,
        correlation_id: CorrelationId,
        genesis_config_hash: Digest,
        protocol_version: ProtocolVersion,
        ee_config: &ExecConfig,
        chainspec_registry: ChainspecRegistry,
    ) -> Result<GenesisSuccess, Error> {
        // Preliminaries
        let initial_root_hash = self.state.empty_root();

        let tracking_copy = match self.tracking_copy(initial_root_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            // NOTE: As genesis is run once per instance condition below is considered programming
            // error
            Ok(None) => panic!("state has not been initialized properly"),
            Err(error) => return Err(error),
        };

        let mut genesis_installer: GenesisInstaller<S> = GenesisInstaller::new(
            genesis_config_hash,
            protocol_version,
            correlation_id,
            ee_config.clone(),
            tracking_copy,
        );

        genesis_installer.install(chainspec_registry)?;

        // Commit the transforms.
        let execution_effect = genesis_installer.finalize();

        let post_state_hash = self
            .state
            .commit(
                correlation_id,
                initial_root_hash,
                execution_effect.transforms.to_owned(),
            )
            .map_err(Into::<execution::Error>::into)?;

        // Return the result
        Ok(GenesisSuccess {
            post_state_hash,
            execution_effect,
        })
    }

    /// Commits upgrade.
    ///
    /// This process applies changes to the global state.
    ///
    /// Returns [`UpgradeSuccess`].
    pub fn commit_upgrade(
        &self,
        correlation_id: CorrelationId,
        upgrade_config: UpgradeConfig,
    ) -> Result<UpgradeSuccess, Error> {
        // per specification:
        // https://casperlabs.atlassian.net/wiki/spaces/EN/pages/139854367/Upgrading+System+Contracts+Specification

        // 3.1.1.1.1.1 validate pre state hash exists
        // 3.1.2.1 get a tracking_copy at the provided pre_state_hash
        let pre_state_hash = upgrade_config.pre_state_hash();
        let tracking_copy = match self.tracking_copy(pre_state_hash)? {
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
            None => return Err(Error::RootNotFound(pre_state_hash)),
        };

        // 3.1.1.1.1.2 current protocol version is required
        let current_protocol_version = upgrade_config.current_protocol_version();

        // 3.1.1.1.1.3 activation point is not currently used by EE; skipping
        // 3.1.1.1.1.4 upgrade point protocol version validation
        let new_protocol_version = upgrade_config.new_protocol_version();

        let upgrade_check_result =
            current_protocol_version.check_next_version(&new_protocol_version);

        if upgrade_check_result.is_invalid() {
            return Err(Error::InvalidProtocolVersion(new_protocol_version));
        }

        let registry = if let Ok(registry) = tracking_copy
            .borrow_mut()
            .get_system_contracts(correlation_id)
        {
            registry
        } else {
            // Check the upgrade config for the registry
            let upgrade_registry = upgrade_config
                .global_state_update()
                .get(&Key::SystemContractRegistry)
                .ok_or_else(|| {
                    error!("Registry is absent in upgrade config");
                    Error::ProtocolUpgrade(ProtocolUpgradeError::FailedToCreateSystemRegistry)
                })?
                .to_owned();
            if let StoredValue::CLValue(cl_registry) = upgrade_registry {
                CLValue::into_t::<SystemContractRegistry>(cl_registry).map_err(|error| {
                    let error_msg = format!("Conversion to system registry failed: {:?}", error);
                    error!("{}", error_msg);
                    Error::Bytesrepr(error_msg)
                })?
            } else {
                error!("Failed to create registry as StoreValue in upgrade config is not CLValue");
                return Err(Error::ProtocolUpgrade(
                    ProtocolUpgradeError::FailedToCreateSystemRegistry,
                ));
            }
        };

        let mint_hash = registry.get(MINT).ok_or_else(|| {
            error!("Missing system mint contract hash");
            Error::MissingSystemContractHash(MINT.to_string())
        })?;
        let auction_hash = registry.get(AUCTION).ok_or_else(|| {
            error!("Missing system auction contract hash");
            Error::MissingSystemContractHash(AUCTION.to_string())
        })?;
        let standard_payment_hash = registry.get(STANDARD_PAYMENT).ok_or_else(|| {
            error!("Missing system standard payment contract hash");
            Error::MissingSystemContractHash(STANDARD_PAYMENT.to_string())
        })?;
        let handle_payment_hash = registry.get(HANDLE_PAYMENT).ok_or_else(|| {
            error!("Missing system handle payment contract hash");
            Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
        })?;

        // Write the chainspec registry to global state
        let cl_value_chainspec_registry =
            CLValue::from_t(upgrade_config.chainspec_registry().clone())
                .map_err(|error| Error::Bytesrepr(error.to_string()))?;

        tracking_copy.borrow_mut().write(
            Key::ChainspecRegistry,
            StoredValue::CLValue(cl_value_chainspec_registry),
        );

        // Cycle through the system contracts and update
        // their metadata if there is a change in entry points.
        let system_upgrader: SystemUpgrader<S> = SystemUpgrader::new(
            new_protocol_version,
            current_protocol_version,
            tracking_copy.clone(),
        );

        system_upgrader
            .refresh_system_contracts(
                correlation_id,
                mint_hash,
                auction_hash,
                handle_payment_hash,
                standard_payment_hash,
            )
            .map_err(Error::ProtocolUpgrade)?;

        // 3.1.1.1.1.7 new total validator slots is optional
        if let Some(new_validator_slots) = upgrade_config.new_validator_slots() {
            // 3.1.2.4 if new total validator slots is provided, update auction contract state
            let auction_contract = tracking_copy
                .borrow_mut()
                .get_contract(correlation_id, *auction_hash)?;

            let validator_slots_key = auction_contract.named_keys()[VALIDATOR_SLOTS_KEY];
            let value = StoredValue::CLValue(
                CLValue::from_t(new_validator_slots)
                    .map_err(|_| Error::Bytesrepr("new_validator_slots".to_string()))?,
            );
            tracking_copy.borrow_mut().write(validator_slots_key, value);
        }

        if let Some(new_auction_delay) = upgrade_config.new_auction_delay() {
            let auction_contract = tracking_copy
                .borrow_mut()
                .get_contract(correlation_id, *auction_hash)?;

            let auction_delay_key = auction_contract.named_keys()[AUCTION_DELAY_KEY];
            let value = StoredValue::CLValue(
                CLValue::from_t(new_auction_delay)
                    .map_err(|_| Error::Bytesrepr("new_auction_delay".to_string()))?,
            );
            tracking_copy.borrow_mut().write(auction_delay_key, value);
        }

        if let Some(new_locked_funds_period) = upgrade_config.new_locked_funds_period_millis() {
            let auction_contract = tracking_copy
                .borrow_mut()
                .get_contract(correlation_id, *auction_hash)?;

            let locked_funds_period_key = auction_contract.named_keys()[LOCKED_FUNDS_PERIOD_KEY];
            let value = StoredValue::CLValue(
                CLValue::from_t(new_locked_funds_period)
                    .map_err(|_| Error::Bytesrepr("new_locked_funds_period".to_string()))?,
            );
            tracking_copy
                .borrow_mut()
                .write(locked_funds_period_key, value);
        }

        if let Some(new_unbonding_delay) = upgrade_config.new_unbonding_delay() {
            let auction_contract = tracking_copy
                .borrow_mut()
                .get_contract(correlation_id, *auction_hash)?;

            let unbonding_delay_key = auction_contract.named_keys()[UNBONDING_DELAY_KEY];
            let value = StoredValue::CLValue(
                CLValue::from_t(new_unbonding_delay)
                    .map_err(|_| Error::Bytesrepr("new_unbonding_delay".to_string()))?,
            );
            tracking_copy.borrow_mut().write(unbonding_delay_key, value);
        }

        if let Some(new_round_seigniorage_rate) = upgrade_config.new_round_seigniorage_rate() {
            let new_round_seigniorage_rate: Ratio<U512> = {
                let (numer, denom) = new_round_seigniorage_rate.into();
                Ratio::new(numer.into(), denom.into())
            };

            let mint_contract = tracking_copy
                .borrow_mut()
                .get_contract(correlation_id, *mint_hash)?;

            let locked_funds_period_key = mint_contract.named_keys()[ROUND_SEIGNIORAGE_RATE_KEY];
            let value = StoredValue::CLValue(
                CLValue::from_t(new_round_seigniorage_rate)
                    .map_err(|_| Error::Bytesrepr("new_round_seigniorage_rate".to_string()))?,
            );
            tracking_copy
                .borrow_mut()
                .write(locked_funds_period_key, value);
        }

        // apply the arbitrary modifications
        for (key, value) in upgrade_config.global_state_update() {
            tracking_copy.borrow_mut().write(*key, value.clone());
        }

        let execution_effect = tracking_copy.borrow().effect();

        // commit
        let post_state_hash = self
            .state
            .commit(
                correlation_id,
                pre_state_hash,
                execution_effect.transforms.to_owned(),
            )
            .map_err(Into::into)?;

        // return result and effects
        Ok(UpgradeSuccess {
            post_state_hash,
            execution_effect,
        })
    }

    /// Creates a new tracking copy instance.
    pub fn tracking_copy(&self, hash: Digest) -> Result<Option<TrackingCopy<S::Reader>>, Error> {
        match self.state.checkout(hash).map_err(Into::into)? {
            Some(tc) => Ok(Some(TrackingCopy::new(tc))),
            None => Ok(None),
        }
    }

    /// Executes a query.
    ///
    /// For a given root [`Key`] it does a path lookup through the named keys.
    ///
    /// Returns the value stored under a [`URef`] wrapped in a [`QueryResult`].
    pub fn run_query(
        &self,
        correlation_id: CorrelationId,
        query_request: QueryRequest,
    ) -> Result<QueryResult, Error> {
        let tracking_copy = match self.tracking_copy(query_request.state_hash())? {
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
            None => return Ok(QueryResult::RootNotFound),
        };

        let tracking_copy = tracking_copy.borrow();

        Ok(tracking_copy
            .query(
                correlation_id,
                self.config(),
                query_request.key(),
                query_request.path(),
            )
            .map_err(|err| Error::Exec(err.into()))?
            .into())
    }

    /// Runs a deploy execution request.
    ///
    /// For each deploy stored in the request it will execute it.
    ///
    /// Currently a special shortcut is taken to distinguish a native transfer, from a deploy.
    ///
    /// Return execution results which contains results from each deploy ran.
    pub fn run_execute(
        &self,
        correlation_id: CorrelationId,
        mut exec_request: ExecuteRequest,
    ) -> Result<ExecutionResults, Error> {
        let executor = Executor::new(*self.config());

        let deploys = exec_request.take_deploys();
        let mut results = ExecutionResults::with_capacity(deploys.len());

        for deploy_item in deploys {
            let result = match deploy_item.session {
                ExecutableDeployItem::Transfer { .. } => self.transfer(
                    correlation_id,
                    &executor,
                    exec_request.protocol_version,
                    exec_request.parent_state_hash,
                    BlockTime::new(exec_request.block_time),
                    deploy_item,
                    exec_request.proposer.clone(),
                ),
                _ => self.deploy(
                    correlation_id,
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

    fn get_authorized_account(
        &self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Result<Account, Error> {
        let account: Account = match tracking_copy
            .borrow_mut()
            .get_account(correlation_id, account_hash)
        {
            Ok(account) => account,
            Err(_) => {
                return Err(error::Error::Authorization);
            }
        };

        // Authorize using provided authorization keys
        if !account.can_authorize(authorization_keys) {
            return Err(error::Error::Authorization);
        }

        // Check total key weight against deploy threshold
        if !account.can_deploy_with(authorization_keys) {
            return Err(execution::Error::DeploymentAuthorizationFailure.into());
        }

        Ok(account)
    }

    /// Get the balance of a passed purse referenced by its [`URef`].
    pub fn get_purse_balance(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
        purse_uref: URef,
    ) -> Result<BalanceResult, Error> {
        let tracking_copy = match self.tracking_copy(state_hash)? {
            Some(tracking_copy) => tracking_copy,
            None => return Ok(BalanceResult::RootNotFound),
        };
        let purse_balance_key =
            tracking_copy.get_purse_balance_key(correlation_id, purse_uref.into())?;
        let (balance, proof) =
            tracking_copy.get_purse_balance_with_proof(correlation_id, purse_balance_key)?;
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
        correlation_id: CorrelationId,
        executor: &Executor,
        protocol_version: ProtocolVersion,
        prestate_hash: Digest,
        blocktime: BlockTime,
        deploy_item: DeployItem,
        proposer: PublicKey,
    ) -> Result<ExecutionResult, Error> {
        let tracking_copy = match self.tracking_copy(prestate_hash) {
            Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            Ok(None) => return Err(Error::RootNotFound(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let base_key = Key::Account(deploy_item.address);

        let account_hash = match base_key.into_account() {
            Some(account_addr) => account_addr,
            None => {
                return Ok(ExecutionResult::precondition_failure(
                    error::Error::Authorization,
                ));
            }
        };

        let authorization_keys = deploy_item.authorization_keys;

        let account = match self.get_authorized_account(
            correlation_id,
            account_hash,
            &authorization_keys,
            Rc::clone(&tracking_copy),
        ) {
            Ok(account) => account,
            Err(e) => return Ok(ExecutionResult::precondition_failure(e)),
        };

        let proposer_addr = proposer.to_account_hash();
        let proposer_account = match tracking_copy
            .borrow_mut()
            .get_account(correlation_id, proposer_addr)
        {
            Ok(proposer) => proposer,
            Err(error) => return Ok(ExecutionResult::precondition_failure(Error::Exec(error))),
        };

        let system_contract_registry = tracking_copy
            .borrow_mut()
            .get_system_contracts(correlation_id)?;

        let handle_payment_contract_hash = system_contract_registry
            .get(HANDLE_PAYMENT)
            .ok_or_else(|| {
                error!("Missing system handle payment contract hash");
                Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
            })?;

        let handle_payment_contract = match tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, *handle_payment_contract_hash)
        {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        let mut handle_payment_access_rights =
            handle_payment_contract.extract_access_rights(*handle_payment_contract_hash);

        let gas_limit = Gas::new(U512::from(std::u64::MAX));

        let wasmless_transfer_gas_cost = Gas::new(U512::from(
            self.config().system_config().wasmless_transfer_cost(),
        ));

        let wasmless_transfer_motes = match Motes::from_gas(
            wasmless_transfer_gas_cost,
            WASMLESS_TRANSFER_FIXED_GAS_PRICE,
        ) {
            Some(motes) => motes,
            None => {
                return Ok(ExecutionResult::precondition_failure(
                    Error::GasConversionOverflow,
                ))
            }
        };

        let proposer_main_purse_balance_key = {
            let proposer_main_purse = proposer_account.main_purse();

            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(correlation_id, proposer_main_purse.into())
            {
                Ok(balance_key) => balance_key,
                Err(error) => return Ok(ExecutionResult::precondition_failure(Error::Exec(error))),
            }
        };

        let proposer_purse = proposer_account.main_purse();

        let account_main_purse = account.main_purse();

        let account_main_purse_balance_key = match tracking_copy
            .borrow_mut()
            .get_purse_balance_key(correlation_id, account_main_purse.into())
        {
            Ok(balance_key) => balance_key,
            Err(error) => return Ok(ExecutionResult::precondition_failure(Error::Exec(error))),
        };

        let account_main_purse_balance = match tracking_copy
            .borrow_mut()
            .get_purse_balance(correlation_id, account_main_purse_balance_key)
        {
            Ok(balance_key) => balance_key,
            Err(error) => return Ok(ExecutionResult::precondition_failure(Error::Exec(error))),
        };

        if account_main_purse_balance < wasmless_transfer_motes {
            // We don't have minimum balance to operate and therefore we can't charge for user
            // errors.
            return Ok(ExecutionResult::precondition_failure(
                Error::InsufficientPayment,
            ));
        }

        // Function below creates an ExecutionResult with precomputed effects of "finalize_payment".
        let make_charged_execution_failure = |error| match ExecutionResult::new_payment_code_error(
            error,
            wasmless_transfer_motes,
            account_main_purse_balance,
            wasmless_transfer_gas_cost,
            account_main_purse_balance_key,
            proposer_main_purse_balance_key,
        ) {
            Ok(execution_result) => execution_result,
            Err(error) => ExecutionResult::precondition_failure(error),
        };

        // All wasmless transfer preconditions are met.
        // Any error that occurs in logic below this point would result in a charge for user error.

        let mut runtime_args_builder =
            TransferRuntimeArgsBuilder::new(deploy_item.session.args().clone());

        match runtime_args_builder.transfer_target_mode(correlation_id, Rc::clone(&tracking_copy)) {
            Ok(mode) => match mode {
                TransferTargetMode::Unknown | TransferTargetMode::PurseExists(_) => { /* noop */ }
                TransferTargetMode::CreateAccount(public_key) => {
                    let create_purse_stack = self.get_new_system_call_stack();
                    let (maybe_uref, execution_result): (Option<URef>, ExecutionResult) = executor
                        .call_system_contract(
                            DirectSystemContractCall::CreatePurse,
                            RuntimeArgs::new(), // mint create takes no arguments
                            &account,
                            authorization_keys.clone(),
                            blocktime,
                            deploy_item.deploy_hash,
                            gas_limit,
                            protocol_version,
                            correlation_id,
                            Rc::clone(&tracking_copy),
                            Phase::Session,
                            create_purse_stack,
                            // We're just creating a purse.
                            U512::zero(),
                        );
                    match maybe_uref {
                        Some(main_purse) => {
                            let new_account =
                                Account::create(public_key, Default::default(), main_purse);
                            // write new account
                            tracking_copy
                                .borrow_mut()
                                .write(Key::Account(public_key), StoredValue::Account(new_account));
                        }
                        None => {
                            // This case implies that the execution_result is a failure variant as
                            // implemented inside host_exec().
                            let error = execution_result
                                .take_error()
                                .unwrap_or(Error::InsufficientPayment);
                            return Ok(make_charged_execution_failure(error));
                        }
                    }
                }
            },
            Err(error) => return Ok(make_charged_execution_failure(error)),
        }

        let transfer_args =
            match runtime_args_builder.build(&account, correlation_id, Rc::clone(&tracking_copy)) {
                Ok(transfer_args) => transfer_args,
                Err(error) => return Ok(make_charged_execution_failure(error)),
            };

        let payment_uref;

        // Construct a payment code that will put cost of wasmless payment into payment purse
        let payment_result = {
            // Check source purses minimum balance
            let source_uref = transfer_args.source();
            let source_purse_balance = if source_uref != account_main_purse {
                let source_purse_balance_key = match tracking_copy
                    .borrow_mut()
                    .get_purse_balance_key(correlation_id, Key::URef(source_uref))
                {
                    Ok(purse_balance_key) => purse_balance_key,
                    Err(error) => return Ok(make_charged_execution_failure(Error::Exec(error))),
                };

                match tracking_copy
                    .borrow_mut()
                    .get_purse_balance(correlation_id, source_purse_balance_key)
                {
                    Ok(purse_balance) => purse_balance,
                    Err(error) => return Ok(make_charged_execution_failure(Error::Exec(error))),
                }
            } else {
                // If source purse is main purse then we already have the balance.
                account_main_purse_balance
            };

            let transfer_amount_motes = Motes::new(transfer_args.amount());

            match wasmless_transfer_motes.checked_add(transfer_amount_motes) {
                Some(total_amount) if source_purse_balance < total_amount => {
                    // We can't continue if the minimum funds in source purse are lower than the
                    // required cost.
                    return Ok(make_charged_execution_failure(Error::InsufficientPayment));
                }
                None => {
                    // When trying to send too much that could cause an overflow.
                    return Ok(make_charged_execution_failure(Error::InsufficientPayment));
                }
                Some(_) => {}
            }

            let get_payment_purse_stack = self.get_new_system_call_stack();
            let (maybe_payment_uref, get_payment_purse_result): (Option<URef>, ExecutionResult) =
                executor.call_system_contract(
                    DirectSystemContractCall::GetPaymentPurse,
                    RuntimeArgs::default(),
                    &account,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_item.deploy_hash,
                    gas_limit,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    Phase::Payment,
                    get_payment_purse_stack,
                    // Getting payment purse does not require transfering tokens.
                    U512::zero(),
                );

            payment_uref = match maybe_payment_uref {
                Some(payment_uref) => payment_uref,
                None => return Ok(make_charged_execution_failure(Error::InsufficientPayment)),
            };

            if let Some(error) = get_payment_purse_result.take_error() {
                return Ok(make_charged_execution_failure(error));
            }

            // Create a new arguments to transfer cost of wasmless transfer into the payment purse.

            let new_transfer_args = TransferArgs::new(
                transfer_args.to(),
                transfer_args.source(),
                payment_uref,
                wasmless_transfer_motes.value(),
                transfer_args.arg_id(),
            );

            let runtime_args = match RuntimeArgs::try_from(new_transfer_args) {
                Ok(runtime_args) => runtime_args,
                Err(error) => return Ok(make_charged_execution_failure(Error::Exec(error.into()))),
            };

            let transfer_to_payment_purse_stack = self.get_new_system_call_stack();
            let (actual_result, payment_result): (Option<Result<(), u8>>, ExecutionResult) =
                executor.call_system_contract(
                    DirectSystemContractCall::Transfer,
                    runtime_args,
                    &account,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_item.deploy_hash,
                    gas_limit,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    Phase::Payment,
                    transfer_to_payment_purse_stack,
                    // We should use only as much as transfer costs.
                    // We're not changing the allowed spending limit since this is a system cost.
                    wasmless_transfer_motes.value(),
                );

            if let Some(error) = payment_result.as_error().cloned() {
                return Ok(make_charged_execution_failure(error));
            }

            let transfer_result = match actual_result {
                Some(Ok(())) => Ok(()),
                Some(Err(mint_error)) => match mint::Error::try_from(mint_error) {
                    Ok(mint_error) => Err(ApiError::from(mint_error)),
                    Err(_) => Err(ApiError::Transfer),
                },
                None => Err(ApiError::Transfer),
            };

            if let Err(error) = transfer_result {
                return Ok(make_charged_execution_failure(Error::Exec(
                    ExecError::Revert(error),
                )));
            }

            let payment_purse_balance = {
                let payment_purse_balance_key = match tracking_copy
                    .borrow_mut()
                    .get_purse_balance_key(correlation_id, Key::URef(payment_uref))
                {
                    Ok(payment_purse_balance_key) => payment_purse_balance_key,
                    Err(error) => return Ok(make_charged_execution_failure(Error::Exec(error))),
                };

                match tracking_copy
                    .borrow_mut()
                    .get_purse_balance(correlation_id, payment_purse_balance_key)
                {
                    Ok(payment_purse_balance) => payment_purse_balance,
                    Err(error) => return Ok(make_charged_execution_failure(Error::Exec(error))),
                }
            };

            // Wasmless transfer payment code pre & post conditions:
            // (a) payment purse should be empty before the payment operation
            // (b) after executing payment code it's balance has to be equal to the wasmless gas
            // cost price

            let payment_gas =
                match Gas::from_motes(payment_purse_balance, WASMLESS_TRANSFER_FIXED_GAS_PRICE) {
                    Some(gas) => gas,
                    None => {
                        return Ok(make_charged_execution_failure(Error::GasConversionOverflow))
                    }
                };

            debug_assert_eq!(payment_gas, wasmless_transfer_gas_cost);

            // This assumes the cost incurred is already denominated in gas

            payment_result.with_cost(payment_gas)
        };

        let runtime_args = match RuntimeArgs::try_from(transfer_args) {
            Ok(runtime_args) => runtime_args,
            Err(error) => {
                return Ok(make_charged_execution_failure(
                    ExecError::from(error).into(),
                ))
            }
        };

        let transfer_stack = self.get_new_system_call_stack();
        let (_, mut session_result): (Option<Result<(), u8>>, ExecutionResult) = executor
            .call_system_contract(
                DirectSystemContractCall::Transfer,
                runtime_args,
                &account,
                authorization_keys.clone(),
                blocktime,
                deploy_item.deploy_hash,
                gas_limit,
                protocol_version,
                correlation_id,
                Rc::clone(&tracking_copy),
                Phase::Session,
                transfer_stack,
                // We limit native transfer to the amount that user signed over as `amount`
                // argument.
                transfer_args.amount(),
            );

        // User is already charged fee for wasmless contract, and we need to make sure we will not
        // charge for anything that happens while calling transfer entrypoint.
        session_result = session_result.with_cost(Gas::default());

        let finalize_result = {
            let handle_payment_args = {
                // Gas spent during payment code execution
                let finalize_cost_motes = {
                    // A case where payment_result.cost() is different than wasmless transfer cost
                    // is considered a programming error.
                    debug_assert_eq!(payment_result.cost(), wasmless_transfer_gas_cost);
                    wasmless_transfer_motes
                };

                let account = deploy_item.address;
                let maybe_runtime_args = RuntimeArgs::try_new(|args| {
                    args.insert(handle_payment::ARG_AMOUNT, finalize_cost_motes.value())?;
                    args.insert(handle_payment::ARG_ACCOUNT, account)?;
                    args.insert(handle_payment::ARG_TARGET, proposer_purse)?;
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

            let system_account = Account::new(
                PublicKey::System.to_account_hash(),
                Default::default(),
                URef::new(Default::default(), AccessRights::READ_ADD_WRITE),
                Default::default(),
                Default::default(),
            );

            let tc = tracking_copy.borrow();
            let finalization_tc = Rc::new(RefCell::new(tc.fork()));

            let finalize_payment_stack = self.get_new_system_call_stack();
            handle_payment_access_rights.extend(&[payment_uref, proposer_purse]);

            let (_ret, finalize_result): (Option<()>, ExecutionResult) = executor
                .call_system_contract(
                    DirectSystemContractCall::FinalizePayment,
                    handle_payment_args,
                    &system_account,
                    authorization_keys,
                    blocktime,
                    deploy_item.deploy_hash,
                    gas_limit,
                    protocol_version,
                    correlation_id,
                    finalization_tc,
                    Phase::FinalizePayment,
                    finalize_payment_stack,
                    // Spending limit is cost of wasmless execution.
                    U512::from(self.config().system_config().wasmless_transfer_cost()),
                );

            finalize_result
        };

        // Create + persist deploy info.
        {
            let transfers = session_result.transfers();
            let cost = wasmless_transfer_gas_cost.value();
            let deploy_info = DeployInfo::new(
                deploy_item.deploy_hash,
                transfers,
                account.account_hash(),
                account.main_purse(),
                cost,
            );
            tracking_copy.borrow_mut().write(
                Key::DeployInfo(deploy_item.deploy_hash),
                StoredValue::DeployInfo(deploy_info),
            );
        }

        if session_result.is_success() {
            session_result = session_result.with_journal(tracking_copy.borrow().execution_journal())
        }

        let mut execution_result_builder = ExecutionResultBuilder::new();
        execution_result_builder.set_payment_execution_result(payment_result);
        execution_result_builder.set_session_execution_result(session_result);
        execution_result_builder.set_finalize_execution_result(finalize_result);

        let execution_result = execution_result_builder
            .build()
            .expect("ExecutionResultBuilder not initialized properly");

        Ok(execution_result)
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
        correlation_id: CorrelationId,
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
            Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            Ok(None) => return Err(Error::RootNotFound(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        // Get addr bytes from `address` (which is actually a Key)
        // validation_spec_3: account validity

        let authorization_keys = deploy_item.authorization_keys;

        // Get account from tracking copy
        // validation_spec_3: account validity
        let account = {
            let account_hash = deploy_item.address;
            match self.get_authorized_account(
                correlation_id,
                account_hash,
                &authorization_keys,
                Rc::clone(&tracking_copy),
            ) {
                Ok(account) => account,
                Err(e) => return Ok(ExecutionResult::precondition_failure(e)),
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
            account.named_keys(),
            session,
            correlation_id,
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
        let account_main_purse_balance_key: Key = {
            let account_key = Key::URef(account.main_purse());
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(correlation_id, account_key)
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
            .get_purse_balance(correlation_id, account_main_purse_balance_key)
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
        let system_account = Account::new(
            PublicKey::System.to_account_hash(),
            Default::default(),
            URef::new(Default::default(), AccessRights::READ_ADD_WRITE),
            Default::default(),
            Default::default(),
        );

        // [`ExecutionResultBuilder`] handles merging of multiple execution results
        let mut execution_result_builder = execution_result::ExecutionResultBuilder::new();

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
            let payment_access_rights = account.extract_access_rights();

            let mut payment_named_keys = account.named_keys().clone();

            let payment_args = payment.args().clone();

            if payment.is_standard_payment(phase) {
                // Todo potentially could be moved to Executor::Exec
                executor.exec_standard_payment(
                    payment_args,
                    Key::Account(account.account_hash()),
                    &account,
                    &mut payment_named_keys,
                    payment_access_rights,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_hash,
                    payment_gas_limit,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    phase,
                    payment_stack,
                )
            } else {
                let payment_execution_kind = match ExecutionKind::new(
                    Rc::clone(&tracking_copy),
                    account.named_keys(),
                    payment,
                    correlation_id,
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
                    &account,
                    &mut payment_named_keys,
                    payment_access_rights,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_hash,
                    payment_gas_limit,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    phase,
                    payment_stack,
                )
            }
        };

        debug!("Payment result: {:?}", payment_result);

        // the proposer of the block this deploy is in receives the gas from this deploy execution
        let proposer_purse = {
            let proposer_account: Account = match tracking_copy
                .borrow_mut()
                .get_account(correlation_id, AccountHash::from(&proposer))
            {
                Ok(account) => account,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            };
            proposer_account.main_purse()
        };

        let proposer_main_purse_balance_key = {
            // Get reward purse Key from handle payment contract
            // payment_code_spec_6: system contract validity
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(correlation_id, proposer_purse.into())
            {
                Ok(key) => key,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        // If provided wasm file was malformed, we should charge.
        if should_charge_for_errors_in_wasm(&payment_result) {
            let error = payment_result
                .as_error()
                .cloned()
                .unwrap_or(Error::InsufficientPayment);

            match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                payment_result.cost(),
                account_main_purse_balance_key,
                proposer_main_purse_balance_key,
            ) {
                Ok(execution_result) => return Ok(execution_result),
                Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            }
        }

        let payment_result_cost = payment_result.cost();
        // payment_code_spec_3: fork based upon payment purse balance and cost of
        // payment code execution

        // Get handle payment system contract details
        // payment_code_spec_6: system contract validity
        let system_contract_registry = tracking_copy
            .borrow_mut()
            .get_system_contracts(correlation_id)?;

        let handle_payment_contract_hash = system_contract_registry
            .get(HANDLE_PAYMENT)
            .ok_or_else(|| {
                error!("Missing system handle payment contract hash");
                Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
            })?;

        let handle_payment_contract = match tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, *handle_payment_contract_hash)
        {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        // Get payment purse Key from handle payment contract
        // payment_code_spec_6: system contract validity
        let payment_purse_key: Key = match handle_payment_contract
            .named_keys()
            .get(handle_payment::PAYMENT_PURSE_KEY)
        {
            Some(key) => *key,
            None => return Ok(ExecutionResult::precondition_failure(Error::Deploy)),
        };
        let purse_balance_key = match tracking_copy
            .borrow_mut()
            .get_purse_balance_key(correlation_id, payment_purse_key)
        {
            Ok(key) => key,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };
        let payment_purse_balance: Motes = {
            match tracking_copy
                .borrow_mut()
                .get_purse_balance(correlation_id, purse_balance_key)
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

            match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                gas_cost,
                account_main_purse_balance_key,
                proposer_main_purse_balance_key,
            ) {
                Ok(execution_result) => return Ok(execution_result),
                Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            }
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

        let session_access_rights = account.extract_access_rights();

        let mut session_named_keys = account.named_keys().clone();

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
                &account,
                &mut session_named_keys,
                session_access_rights,
                authorization_keys.clone(),
                blocktime,
                deploy_hash,
                session_gas_limit,
                protocol_version,
                correlation_id,
                Rc::clone(&session_tracking_copy),
                Phase::Session,
                session_stack,
            )
        };
        debug!("Session result: {:?}", session_result);

        // Create + persist deploy info.
        {
            let transfers = session_result.transfers();
            let cost = payment_result_cost.value() + session_result.cost().value();
            let deploy_info = DeployInfo::new(
                deploy_hash,
                transfers,
                account.account_hash(),
                account.main_purse(),
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

            match ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                session_result.cost(),
                account_main_purse_balance_key,
                proposer_main_purse_balance_key,
            ) {
                Ok(execution_result) => return Ok(execution_result),
                Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            }
        }

        let post_session_rc = if session_result.is_failure() {
            // If session code fails we do not include its effects,
            // so we start again from the post-payment state.
            Rc::new(RefCell::new(post_payment_tracking_copy.fork()))
        } else {
            session_result =
                session_result.with_journal(session_tracking_copy.borrow().execution_journal());
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
                    args.insert(handle_payment::ARG_ACCOUNT, account.account_hash())?;
                    args.insert(handle_payment::ARG_TARGET, proposer_purse)?;
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
            let system_contract_registry = finalization_tc
                .borrow_mut()
                .get_system_contracts(correlation_id)?;

            let handle_payment_contract_hash = system_contract_registry
                .get(HANDLE_PAYMENT)
                .ok_or_else(|| {
                    error!("Missing system handle payment contract hash");
                    Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
                })?;

            let handle_payment_contract = match finalization_tc
                .borrow_mut()
                .get_contract(correlation_id, *handle_payment_contract_hash)
            {
                Ok(info) => info,
                Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
            };

            let mut handle_payment_access_rights =
                handle_payment_contract.extract_access_rights(*handle_payment_contract_hash);
            handle_payment_access_rights.extend(&[
                payment_purse_key
                    .into_uref()
                    .ok_or(Error::InvalidKeyVariant)?,
                proposer_purse,
            ]);

            let gas_limit = Gas::new(U512::MAX);

            let handle_payment_stack = self.get_new_system_call_stack();

            let (_ret, finalize_result): (Option<()>, ExecutionResult) = executor
                .call_system_contract(
                    DirectSystemContractCall::FinalizePayment,
                    handle_payment_args,
                    &system_account,
                    authorization_keys,
                    blocktime,
                    deploy_hash,
                    gas_limit,
                    protocol_version,
                    correlation_id,
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

    /// Apply effects of the execution.
    ///
    /// This is also referred to as "committing" the effects into the global state. This method has
    /// to be run after an execution has been made to persists the effects of it.
    ///
    /// Returns new state root hash.
    pub fn apply_effect(
        &self,
        correlation_id: CorrelationId,
        pre_state_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Error> {
        self.state
            .commit(correlation_id, pre_state_hash, effects)
            .map_err(|err| Error::Exec(err.into()))
    }

    /// Gets a trie (or chunk) object for given state root hash.
    pub fn get_trie(
        &self,
        correlation_id: CorrelationId,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, Error>
    where
        Error: From<S::Error>,
    {
        Ok(self.state.get_trie(correlation_id, trie_or_chunk_id)?)
    }

    /// Gets a trie object for given state root hash.
    pub fn get_trie_full(
        &self,
        correlation_id: CorrelationId,
        trie_key: Digest,
    ) -> Result<Option<Bytes>, Error>
    where
        Error: From<S::Error>,
    {
        Ok(self.state.get_trie_full(correlation_id, &trie_key)?)
    }

    /// Puts a trie and finds missing descendant trie keys.
    pub fn put_trie_and_find_missing_descendant_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_bytes: &[u8],
    ) -> Result<Vec<Digest>, Error>
    where
        Error: From<S::Error>,
    {
        let inserted_trie_key = self.state.put_trie(correlation_id, trie_bytes)?;
        let missing_descendant_trie_keys = self
            .state
            .missing_trie_keys(correlation_id, vec![inserted_trie_key])?;
        Ok(missing_descendant_trie_keys)
    }

    /// Performs a lookup for a list of missing root hashes.
    pub fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Error>
    where
        Error: From<S::Error>,
    {
        self.state
            .missing_trie_keys(correlation_id, trie_keys)
            .map_err(Error::from)
    }

    /// Obtains validator weights for given era.
    ///
    /// This skips execution of auction's `get_era_validator` entry point logic to avoid creating an
    /// executor instance, and going through the execution flow. It follows the same process but
    /// uses queries rather than execution to get the snapshot.
    pub fn get_era_validators(
        &self,
        correlation_id: CorrelationId,
        system_contract_registry: Option<SystemContractRegistry>,
        get_era_validators_request: GetEraValidatorsRequest,
    ) -> Result<EraValidators, GetEraValidatorsError> {
        let state_root_hash = get_era_validators_request.state_hash();

        let system_contract_registry = match system_contract_registry {
            Some(system_contract_registry) => system_contract_registry,
            None => match self.get_system_contract_registry(correlation_id, state_root_hash) {
                Ok(system_contract_registry) => system_contract_registry,
                Err(error) => {
                    error!(%state_root_hash, %error, "unable to get era validators");
                    return Err(error.into());
                }
            },
        };

        let auction_hash = system_contract_registry
            .get(AUCTION)
            .copied()
            .ok_or_else(|| Error::MissingSystemContractHash(AUCTION.to_string()))?;

        let query_request = QueryRequest::new(
            state_root_hash,
            auction_hash.into(),
            vec![SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.to_string()],
        );

        let snapshot = match self.run_query(correlation_id, query_request)? {
            QueryResult::RootNotFound => return Err(GetEraValidatorsError::RootNotFound),
            QueryResult::ValueNotFound(error) => {
                error!(%error, "unexpected query failure; value not found");
                return Err(GetEraValidatorsError::EraValidatorsMissing);
            }
            QueryResult::CircularReference(error) => {
                error!(%error, "unexpected query failure; circular reference");
                return Err(GetEraValidatorsError::UnexpectedQueryFailure);
            }
            QueryResult::DepthLimit { depth } => {
                error!(%depth, "unexpected query failure; depth limit exceeded");
                return Err(GetEraValidatorsError::UnexpectedQueryFailure);
            }
            QueryResult::Success { value, proofs: _ } => {
                let cl_value = match value.as_cl_value() {
                    Some(snapshot_cl_value) => snapshot_cl_value.clone(),
                    None => {
                        error!("unexpected query failure; seigniorage recipients snapshot is not a CLValue");
                        return Err(GetEraValidatorsError::UnexpectedQueryFailure);
                    }
                };

                cl_value.into_t().map_err(|cl_value_error| {
                    error!(%cl_value_error, "unexpected query failure; unable to parse seigniorage recipients");
                    GetEraValidatorsError::CLValue
                })?
            }
        };

        let era_validators_result = auction::detail::era_validators_from_snapshot(snapshot);
        Ok(era_validators_result)
    }

    /// Gets current bids from the auction system.
    pub fn get_bids(
        &self,
        correlation_id: CorrelationId,
        get_bids_request: GetBidsRequest,
    ) -> Result<GetBidsResult, Error> {
        let tracking_copy = match self.tracking_copy(get_bids_request.state_hash())? {
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
            None => return Ok(GetBidsResult::RootNotFound),
        };

        let mut tracking_copy = tracking_copy.borrow_mut();

        let bid_keys = tracking_copy
            .get_keys(correlation_id, &KeyTag::Bid)
            .map_err(|err| Error::Exec(err.into()))?;

        let mut bids = BTreeMap::new();

        for key in bid_keys.iter() {
            if let Some(StoredValue::Bid(bid)) =
                tracking_copy.get(correlation_id, key).map_err(Into::into)?
            {
                bids.insert(bid.validator_public_key().clone(), *bid);
            };
        }

        Ok(GetBidsResult::Success { bids })
    }

    /// Distribute block rewards.
    pub fn distribute_block_rewards(
        &self,
        correlation_id: CorrelationId,
        pre_state_hash: Digest,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
        next_block_height: u64,
        time: u64,
    ) -> Result<Digest, StepError> {
        let tracking_copy = match self.tracking_copy(pre_state_hash) {
            Err(error) => return Err(StepError::TrackingCopyError(error)),
            Ok(None) => return Err(StepError::RootNotFound(pre_state_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert(ARG_VALIDATOR, proposer)?;

        let executor = Executor::new(*self.config());

        let system_account_addr = PublicKey::System.to_account_hash();

        let virtual_system_account = {
            let named_keys = NamedKeys::new();
            let purse = URef::new(Default::default(), AccessRights::READ_ADD_WRITE);
            Account::create(system_account_addr, named_keys, purse)
        };

        let authorization_keys = {
            let mut ret = BTreeSet::new();
            ret.insert(system_account_addr);
            ret
        };

        let gas_limit = Gas::new(U512::from(std::u64::MAX));

        let deploy_hash = {
            // seeds address generator w/ era_end_timestamp_millis
            let mut bytes = time.into_bytes()?;
            bytes.append(&mut next_block_height.into_bytes()?);
            DeployHash::new(Digest::hash(&bytes).value())
        };

        let distribute_rewards_stack = self.get_new_system_call_stack();
        let (_, execution_result): (Option<()>, ExecutionResult) = executor.call_system_contract(
            DirectSystemContractCall::DistributeRewards,
            runtime_args,
            &virtual_system_account,
            authorization_keys,
            BlockTime::default(),
            deploy_hash,
            gas_limit,
            protocol_version,
            correlation_id,
            Rc::clone(&tracking_copy),
            Phase::Session,
            distribute_rewards_stack,
            // There should be no tokens transferred during rewards distribution.
            U512::zero(),
        );

        if let Some(exec_error) = execution_result.take_error() {
            return Err(StepError::DistributeError(exec_error));
        }

        let execution_effect = tracking_copy.borrow().effect();

        // commit
        let post_state_hash = self
            .state
            .commit(correlation_id, pre_state_hash, execution_effect.transforms)
            .map_err(Into::into)?;

        Ok(post_state_hash)
    }

    /// Executes a step request.
    pub fn commit_step(
        &self,
        correlation_id: CorrelationId,
        step_request: StepRequest,
    ) -> Result<StepSuccess, StepError> {
        let tracking_copy = match self.tracking_copy(step_request.pre_state_hash) {
            Err(error) => return Err(StepError::TrackingCopyError(error)),
            Ok(None) => return Err(StepError::RootNotFound(step_request.pre_state_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let executor = Executor::new(*self.config());

        let system_account_addr = PublicKey::System.to_account_hash();

        let virtual_system_account = {
            let named_keys = NamedKeys::new();
            let purse = URef::new(Default::default(), AccessRights::READ_ADD_WRITE);
            Account::create(system_account_addr, named_keys, purse)
        };
        let authorization_keys = {
            let mut ret = BTreeSet::new();
            ret.insert(system_account_addr);
            ret
        };

        let gas_limit = Gas::new(U512::from(std::u64::MAX));
        let deploy_hash = {
            // seeds address generator w/ era_end_timestamp_millis
            let mut bytes = step_request.era_end_timestamp_millis.into_bytes()?;
            bytes.append(&mut step_request.next_era_id.into_bytes()?);
            DeployHash::new(Digest::hash(&bytes).value())
        };

        let slashed_validators: Vec<PublicKey> = step_request.slashed_validators();

        if !slashed_validators.is_empty() {
            let slash_args = {
                let mut runtime_args = RuntimeArgs::new();
                runtime_args
                    .insert(ARG_VALIDATOR_PUBLIC_KEYS, slashed_validators)
                    .map_err(|e| Error::Exec(e.into()))?;
                runtime_args
            };

            let slash_stack = self.get_new_system_call_stack();
            let (_, execution_result): (Option<()>, ExecutionResult) = executor
                .call_system_contract(
                    DirectSystemContractCall::Slash,
                    slash_args,
                    &virtual_system_account,
                    authorization_keys.clone(),
                    BlockTime::default(),
                    deploy_hash,
                    gas_limit,
                    step_request.protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    Phase::Session,
                    slash_stack,
                    // No transfer should occur when slashing.
                    U512::zero(),
                );

            if let Some(exec_error) = execution_result.take_error() {
                return Err(StepError::SlashingError(exec_error));
            }
        }

        let run_auction_args = RuntimeArgs::try_new(|args| {
            args.insert(
                ARG_ERA_END_TIMESTAMP_MILLIS,
                step_request.era_end_timestamp_millis,
            )?;
            args.insert(
                ARG_EVICTED_VALIDATORS,
                step_request
                    .evict_items
                    .iter()
                    .map(|item| item.validator_id.clone())
                    .collect::<Vec<PublicKey>>(),
            )?;
            Ok(())
        })?;

        let run_auction_stack = self.get_new_system_call_stack();
        let (_, execution_result): (Option<()>, ExecutionResult) = executor.call_system_contract(
            DirectSystemContractCall::RunAuction,
            run_auction_args,
            &virtual_system_account,
            authorization_keys,
            BlockTime::default(),
            deploy_hash,
            gas_limit,
            step_request.protocol_version,
            correlation_id,
            Rc::clone(&tracking_copy),
            Phase::Session,
            run_auction_stack,
            // RunAuction should not consume tokens.
            U512::zero(),
        );

        if let Some(exec_error) = execution_result.take_error() {
            return Err(StepError::AuctionError(exec_error));
        }

        let execution_effect = tracking_copy.borrow().effect();
        let execution_journal = tracking_copy.borrow().execution_journal();

        // commit
        let post_state_hash = self
            .state
            .commit(
                correlation_id,
                step_request.pre_state_hash,
                execution_effect.transforms,
            )
            .map_err(Into::into)?;

        Ok(StepSuccess {
            post_state_hash,
            execution_journal,
        })
    }

    /// Gets the balance of a given public key.
    pub fn get_balance(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
        public_key: PublicKey,
    ) -> Result<BalanceResult, Error> {
        // Look up the account, get the main purse, and then do the existing balance check
        let tracking_copy = match self.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return Ok(BalanceResult::RootNotFound),
            Err(error) => return Err(error),
        };

        let account_addr = public_key.to_account_hash();

        let account = match tracking_copy
            .borrow_mut()
            .get_account(correlation_id, account_addr)
        {
            Ok(account) => account,
            Err(error) => return Err(error.into()),
        };

        let main_purse_balance_key = {
            let main_purse = account.main_purse();
            match tracking_copy
                .borrow()
                .get_purse_balance_key(correlation_id, main_purse.into())
            {
                Ok(balance_key) => balance_key,
                Err(error) => return Err(error.into()),
            }
        };

        let (account_balance, proof) = match tracking_copy
            .borrow()
            .get_purse_balance_with_proof(correlation_id, main_purse_balance_key)
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
        correlation_id: CorrelationId,
        state_root_hash: Digest,
    ) -> Result<SystemContractRegistry, Error> {
        let tracking_copy = match self.tracking_copy(state_root_hash)? {
            None => return Err(Error::RootNotFound(state_root_hash)),
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
        };
        let result = tracking_copy
            .borrow_mut()
            .get_system_contracts(correlation_id)
            .map_err(|error| {
                error!(%error, "Failed to retrieve system contract registry");
                Error::MissingSystemContractRegistry
            });
        result
    }

    /// Returns mint system contract hash.
    pub fn get_system_mint_hash(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
    ) -> Result<ContractHash, Error> {
        let registry = self.get_system_contract_registry(correlation_id, state_hash)?;
        let mint_hash = registry.get(MINT).ok_or_else(|| {
            error!("Missing system mint contract hash");
            Error::MissingSystemContractHash(MINT.to_string())
        })?;
        Ok(*mint_hash)
    }

    /// Returns auction system contract hash.
    pub fn get_system_auction_hash(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
    ) -> Result<ContractHash, Error> {
        let registry = self.get_system_contract_registry(correlation_id, state_hash)?;
        let auction_hash = registry.get(AUCTION).ok_or_else(|| {
            error!("Missing system auction contract hash");
            Error::MissingSystemContractHash(AUCTION.to_string())
        })?;
        Ok(*auction_hash)
    }

    /// Returns handle payment system contract hash.
    pub fn get_handle_payment_hash(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
    ) -> Result<ContractHash, Error> {
        let registry = self.get_system_contract_registry(correlation_id, state_hash)?;
        let handle_payment = registry.get(HANDLE_PAYMENT).ok_or_else(|| {
            error!("Missing system handle payment contract hash");
            Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
        })?;
        Ok(*handle_payment)
    }

    /// Returns standard payment system contract hash.
    pub fn get_standard_payment_hash(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
    ) -> Result<ContractHash, Error> {
        let registry = self.get_system_contract_registry(correlation_id, state_hash)?;
        let standard_payment = registry.get(STANDARD_PAYMENT).ok_or_else(|| {
            error!("Missing system standard payment contract hash");
            Error::MissingSystemContractHash(STANDARD_PAYMENT.to_string())
        })?;
        Ok(*standard_payment)
    }

    fn get_new_system_call_stack(&self) -> RuntimeStack {
        let max_height = self.config.max_runtime_call_stack_height() as usize;
        RuntimeStack::new_system_call_stack(max_height)
    }
}

fn should_charge_for_errors_in_wasm(execution_result: &ExecutionResult) -> bool {
    match execution_result {
        ExecutionResult::Failure {
            error,
            transfers: _,
            cost: _,
            execution_journal: _,
        } => match error {
            Error::Exec(err) => match err {
                ExecError::WasmPreprocessing(_) | ExecError::UnsupportedWasmStart => true,
                ExecError::Storage(_)
                | ExecError::InvalidContractWasm(_)
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
                | ExecError::ExpectedReturnValue
                | ExecError::UnexpectedReturnValue
                | ExecError::InvalidContext
                | ExecError::IncompatibleProtocolMajorVersion { .. }
                | ExecError::CLValue(_)
                | ExecError::HostBufferEmpty
                | ExecError::NoActiveContractVersions(_)
                | ExecError::InvalidContractVersion(_)
                | ExecError::NoSuchMethod(_)
                | ExecError::KeyIsNotAURef(_)
                | ExecError::UnexpectedStoredValueVariant
                | ExecError::LockedContract(_)
                | ExecError::InvalidContractPackage(_)
                | ExecError::InvalidContract(_)
                | ExecError::MissingArgument { .. }
                | ExecError::DictionaryItemKeyExceedsLength
                | ExecError::MissingSystemContractRegistry
                | ExecError::MissingSystemContractHash(_)
                | ExecError::RuntimeStackOverflow
                | ExecError::ValueTooLarge
                | ExecError::MissingRuntimeStack
                | ExecError::DisabledContract(_) => false,
            },
            Error::WasmPreprocessing(_) => true,
            Error::WasmSerialization(_) => true,
            Error::RootNotFound(_)
            | Error::InvalidProtocolVersion(_)
            | Error::Genesis(_)
            | Error::Storage(_)
            | Error::Authorization
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
            | Error::RuntimeStackOverflow
            | Error::FailedToGetWithdrawKeys
            | Error::FailedToGetStoredWithdraws
            | Error::FailedToGetWithdrawPurses
            | Error::FailedToRetrieveUnbondingDelay
            | Error::FailedToRetrieveEraId => false,
        },
        ExecutionResult::Success { .. } => false,
    }
}
