//! Contract Runtime component.
pub(crate) mod announcements;
mod config;
mod error;
mod metrics;
mod operations;
mod types;

use once_cell::sync::Lazy;
use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use datasize::DataSize;
use lmdb::DatabaseFlags;
use prometheus::Registry;
use serde::Serialize;
use tracing::{debug, info, trace};

use casper_execution_engine::{
    core::engine_state::{
        self, genesis::GenesisSuccess, EngineConfig, EngineState, GetEraValidatorsError,
        GetEraValidatorsRequest, UpgradeConfig, UpgradeSuccess,
    },
    shared::{newtypes::CorrelationId, system_config::SystemConfig, wasm_config::WasmConfig},
    storage::{
        global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_types::ProtocolVersion;

use crate::{
    components::{contract_runtime::types::StepEffectAndUpcomingEraValidators, Component},
    effect::{
        announcements::ControlAnnouncement, requests::ContractRuntimeRequest, EffectBuilder,
        EffectExt, Effects,
    },
    fatal,
    types::{BlockHash, BlockHeader, Chainspec, Deploy, FinalizedBlock},
    NodeRng,
};
pub(crate) use announcements::ContractRuntimeAnnouncement;
pub(crate) use config::Config;
pub(crate) use error::{BlockExecutionError, ConfigError};
use metrics::Metrics;
pub use operations::execute_finalized_block;
pub use types::BlockAndExecutionEffects;
pub(crate) use types::EraValidatorsRequest;

/// Maximum number of resource intensive tasks that can be run in parallel.
///
/// TODO: Fine tune this constant to the machine executing the node.
const MAX_PARALLEL_INTENSIVE_TASKS: usize = 4;

/// Semaphore enforcing maximum number of parallel resource intensive tasks.
static INTENSIVE_TASKS_SEMAPHORE: Lazy<tokio::sync::Semaphore> =
    Lazy::new(|| tokio::sync::Semaphore::new(MAX_PARALLEL_INTENSIVE_TASKS));

/// Asynchronously runs a resource intensive task.
/// At most `MAX_PARALLEL_INTENSIVE_TASKS` are being run in parallel at any time.
///
/// The task is a closure that takes no arguments and returns a value.
/// This function returns a future for that value.
async fn run_intensive_task<T, V>(task: T) -> V
where
    T: 'static + Send + FnOnce() -> V,
    V: 'static + Send,
{
    // This will never panic since the semaphore is never closed.
    let _permit = INTENSIVE_TASKS_SEMAPHORE.acquire().await.unwrap();
    tokio::task::spawn_blocking(task)
        .await
        .expect("task panicked")
}

/// State to use to construct the next block in the blockchain. Includes the state root hash for the
/// execution engine as well as certain values the next header will be based on.
#[derive(DataSize, Debug, Clone, Serialize)]
pub struct ExecutionPreState {
    /// The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    next_block_height: u64,
    /// The state root to use when executing deploys.
    pre_state_root_hash: Digest,
    /// The parent hash of the next `Block`.
    parent_hash: BlockHash,
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    parent_seed: Digest,
}

impl ExecutionPreState {
    pub(crate) fn new(
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Self {
        ExecutionPreState {
            next_block_height,
            pre_state_root_hash,
            parent_hash,
            parent_seed,
        }
    }

    /// Get the next block height according that will succeed the block specified by `parent_hash`.
    pub(crate) fn next_block_height(&self) -> u64 {
        self.next_block_height
    }
}

impl From<&BlockHeader> for ExecutionPreState {
    fn from(block_header: &BlockHeader) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.hash(),
            parent_seed: block_header.accumulated_seed(),
        }
    }
}

type ExecQueue = Arc<Mutex<BTreeMap<u64, (FinalizedBlock, Vec<Deploy>, Vec<Deploy>)>>>;

/// The contract runtime components.
#[derive(DataSize)]
pub(crate) struct ContractRuntime {
    execution_pre_state: Arc<Mutex<ExecutionPreState>>,
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<Metrics>,
    protocol_version: ProtocolVersion,

    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: ExecQueue,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContractRuntime").finish()
    }
}

impl<REv> Component<REv> for ContractRuntime
where
    REv: From<ContractRuntimeRequest>
        + From<ContractRuntimeAnnouncement>
        + From<ControlAnnouncement>
        + Send,
{
    type Event = ContractRuntimeRequest;
    type ConstructionError = ConfigError;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            ContractRuntimeRequest::CommitGenesis {
                chainspec,
                responder,
            } => {
                let result = self.commit_genesis(&chainspec);
                responder.respond(result).ignore()
            }
            ContractRuntimeRequest::Upgrade {
                upgrade_config,
                responder,
            } => responder
                .respond(self.commit_upgrade(*upgrade_config))
                .ignore(),
            ContractRuntimeRequest::Query {
                query_request,
                responder,
            } => {
                trace!(?query_request, "query");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.run_query(correlation_id, query_request);
                    metrics.run_query.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "query result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetBalance {
                balance_request,
                responder,
            } => {
                trace!(?balance_request, "balance");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.get_purse_balance(
                        correlation_id,
                        balance_request.state_hash(),
                        balance_request.purse_uref(),
                    );
                    metrics.get_balance.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "balance result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::IsBonded {
                state_root_hash,
                era_id,
                protocol_version,
                public_key: validator_key,
                responder,
            } => {
                trace!(era=%era_id, public_key = %validator_key, "is validator bonded request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                let request = GetEraValidatorsRequest::new(state_root_hash, protocol_version);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let era_validators = engine_state.get_era_validators(correlation_id, request);
                    metrics
                        .get_validator_weights
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?era_validators, "is validator bonded result");
                    let is_bonded =
                        era_validators.and_then(|validator_map| match validator_map.get(&era_id) {
                            None => Err(GetEraValidatorsError::EraValidatorsMissing),
                            Some(era_validators) => Ok(era_validators.contains_key(&validator_key)),
                        });
                    responder.respond(is_bonded).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetEraValidators { request, responder } => {
                trace!(?request, "get era validators request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                // Increment the counter to track the amount of times GetEraValidators was
                // requested.
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let era_validators =
                        engine_state.get_era_validators(correlation_id, request.into());
                    metrics
                        .get_era_validators
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?era_validators, "get era validators response");
                    responder.respond(era_validators).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetTrie {
                trie_key,
                responder,
            } => {
                trace!(?trie_key, "get_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.get_trie(correlation_id, trie_key);
                    metrics.get_trie.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get_trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::PutTrie { trie, responder } => {
                trace!(?trie, "put_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state
                        .put_trie_and_find_missing_descendant_trie_keys(correlation_id, &*trie);
                    // PERF: this *could* be called only periodically.
                    if let Err(lmdb_error) = engine_state.flush_environment() {
                        fatal!(
                            effect_builder,
                            "error flushing lmdb environment {:?}",
                            lmdb_error
                        )
                        .await;
                    } else {
                        metrics.put_trie.observe(start.elapsed().as_secs_f64());
                        trace!(?result, "put_trie response");
                        responder.respond(result).await
                    }
                }
                .ignore()
            }
            ContractRuntimeRequest::ExecuteBlock {
                protocol_version,
                execution_pre_state,
                finalized_block,
                deploys,
                transfers,
                responder,
            } => {
                trace!(
                    ?protocol_version,
                    ?execution_pre_state,
                    ?finalized_block,
                    ?deploys,
                    "execute block request"
                );
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let result = run_intensive_task(move || {
                        execute_finalized_block(
                            engine_state.as_ref(),
                            Some(metrics),
                            protocol_version,
                            execution_pre_state,
                            finalized_block,
                            deploys,
                            transfers,
                        )
                    })
                    .await;
                    trace!(?result, "execute block response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::EnqueueBlockForExecution {
                finalized_block,
                deploys,
                transfers,
            } => {
                info!(?finalized_block, "enqueuing finalized block for execution");
                let mut effects = Effects::new();
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                let exec_queue = Arc::clone(&self.exec_queue);
                let execution_pre_state = Arc::clone(&self.execution_pre_state);
                let protocol_version = self.protocol_version;
                if self.execution_pre_state.lock().unwrap().next_block_height
                    == finalized_block.height()
                {
                    effects.extend(
                        Self::execute_finalized_block_or_requeue(
                            engine_state,
                            metrics,
                            exec_queue,
                            execution_pre_state,
                            effect_builder,
                            protocol_version,
                            finalized_block,
                            deploys,
                            transfers,
                        )
                        .ignore(),
                    )
                } else {
                    exec_queue.lock().unwrap().insert(
                        finalized_block.height(),
                        (finalized_block, deploys, transfers),
                    );
                }
                effects
            }
            ContractRuntimeRequest::GetBids {
                get_bids_request,
                responder,
            } => {
                trace!(?get_bids_request, "get bids request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.get_bids(correlation_id, get_bids_request);
                    metrics.get_bids.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get bids result");
                    responder.respond(result).await
                }
                .ignore()
            }
        }
    }
}

impl ContractRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        protocol_version: ProtocolVersion,
        storage_dir: &Path,
        contract_runtime_config: &Config,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
        max_associated_keys: u32,
        max_runtime_call_stack_height: u32,
        max_stored_value_size: u32,
        max_delegator_size_limit: u32,
        minimum_delegation_amount: u64,
        registry: &Registry,
    ) -> Result<Self, ConfigError> {
        // TODO: This is bogus, get rid of this
        let execution_pre_state = Arc::new(Mutex::new(ExecutionPreState {
            pre_state_root_hash: Default::default(),
            next_block_height: 0,
            parent_hash: Default::default(),
            parent_seed: Default::default(),
        }));

        let environment = Arc::new(LmdbEnvironment::new(
            storage_dir,
            contract_runtime_config.max_global_state_size(),
            contract_runtime_config.max_readers(),
            contract_runtime_config.manual_sync_enabled(),
        )?);

        let trie_store = Arc::new(LmdbTrieStore::new(
            &environment,
            None,
            DatabaseFlags::empty(),
        )?);

        let global_state = LmdbGlobalState::empty(environment, trie_store)?;
        let engine_config = EngineConfig::new(
            contract_runtime_config.max_query_depth(),
            max_associated_keys,
            max_runtime_call_stack_height,
            max_stored_value_size,
            max_delegator_size_limit,
            minimum_delegation_amount,
            wasm_config,
            system_config,
        );

        let engine_state = Arc::new(EngineState::new(global_state, engine_config));

        let metrics = Arc::new(Metrics::new(registry)?);

        Ok(ContractRuntime {
            execution_pre_state,
            protocol_version,
            exec_queue: Arc::new(Mutex::new(BTreeMap::new())),
            engine_state,
            metrics,
        })
    }

    /// Commits a genesis using a chainspec
    pub(crate) fn commit_genesis(
        &self,
        chainspec: &Chainspec,
    ) -> Result<GenesisSuccess, engine_state::Error> {
        let correlation_id = CorrelationId::new();
        let genesis_config_hash = chainspec.hash();
        let protocol_version = chainspec.protocol_config.version;
        // Transforms a chainspec into a valid genesis config for execution engine.
        let ee_config = chainspec.into();
        self.engine_state.commit_genesis(
            correlation_id,
            genesis_config_hash,
            protocol_version,
            &ee_config,
        )
    }

    fn commit_upgrade(
        &self,
        upgrade_config: UpgradeConfig,
    ) -> Result<UpgradeSuccess, engine_state::Error> {
        debug!(?upgrade_config, "upgrade");
        let start = Instant::now();
        let result = self
            .engine_state
            .commit_upgrade(CorrelationId::new(), upgrade_config);
        self.metrics
            .commit_upgrade
            .observe(start.elapsed().as_secs_f64());
        debug!(?result, "upgrade result");
        result
    }

    /// Retrieve trie keys for the integrity check.
    pub(crate) fn trie_store_check(
        &self,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, engine_state::Error> {
        let correlation_id = CorrelationId::new();
        let start = Instant::now();
        let result = self
            .engine_state
            .missing_trie_keys(correlation_id, trie_keys);
        self.metrics
            .missing_trie_keys
            .observe(start.elapsed().as_secs_f64());
        result
    }

    pub(crate) fn set_initial_state(&mut self, sequential_block_state: ExecutionPreState) {
        *self.execution_pre_state.lock().unwrap() = sequential_block_state;
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_finalized_block_or_requeue<REv>(
        engine_state: Arc<EngineState<LmdbGlobalState>>,
        metrics: Arc<Metrics>,
        exec_queue: ExecQueue,
        execution_pre_state: Arc<Mutex<ExecutionPreState>>,
        effect_builder: EffectBuilder<REv>,
        protocol_version: ProtocolVersion,
        finalized_block: FinalizedBlock,
        deploys: Vec<Deploy>,
        transfers: Vec<Deploy>,
    ) where
        REv: From<ContractRuntimeRequest>
            + From<ContractRuntimeAnnouncement>
            + From<ControlAnnouncement>
            + Send,
    {
        let current_execution_pre_state = execution_pre_state.lock().unwrap().clone();
        let BlockAndExecutionEffects {
            block,
            execution_results,
            maybe_step_effect_and_upcoming_era_validators,
        } = match run_intensive_task(move || {
            execute_finalized_block(
                engine_state.as_ref(),
                Some(metrics),
                protocol_version,
                current_execution_pre_state,
                finalized_block,
                deploys,
                transfers,
            )
        })
        .await
        {
            Ok(block_and_execution_effects) => block_and_execution_effects,
            Err(error) => return fatal!(effect_builder, "{}", error).await,
        };

        let new_execution_pre_state = ExecutionPreState::from(block.header());
        *execution_pre_state.lock().unwrap() = new_execution_pre_state.clone();

        let current_era_id = block.header().era_id();

        announcements::linear_chain_block(effect_builder, block, execution_results).await;

        if let Some(StepEffectAndUpcomingEraValidators {
            step_execution_journal,
            upcoming_era_validators,
        }) = maybe_step_effect_and_upcoming_era_validators
        {
            announcements::step_success(effect_builder, current_era_id, step_execution_journal)
                .await;

            announcements::upcoming_era_validators(
                effect_builder,
                current_era_id,
                upcoming_era_validators,
            )
            .await;
        }

        // If the child is already finalized, start execution.
        let next_block = {
            // needed to help this async block impl Send (the MutexGuard lives too long)
            let queue = &mut *exec_queue.lock().expect("mutex poisoned");
            queue.remove(&new_execution_pre_state.next_block_height)
        };
        if let Some((finalized_block, deploys, transfers)) = next_block {
            effect_builder
                .enqueue_block_for_execution(finalized_block, deploys, transfers)
                .await
        }
    }

    /// Returns the engine state, for testing only.
    #[cfg(test)]
    pub(crate) fn engine_state(&self) -> &Arc<EngineState<LmdbGlobalState>> {
        &self.engine_state
    }
}
