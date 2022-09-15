//! Contract Runtime component.

mod config;
mod error;
mod metrics;
mod operations;
mod types;

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use lmdb::DatabaseFlags;
use once_cell::sync::Lazy;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, info, trace};

use casper_execution_engine::{
    core::engine_state::{
        self, genesis::GenesisError, ChainspecRegistry, EngineConfig, EngineState, GenesisSuccess,
        GetEraValidatorsError, GetEraValidatorsRequest, SystemContractRegistry, UpgradeConfig,
        UpgradeSuccess,
    },
    shared::{newtypes::CorrelationId, system_config::SystemConfig, wasm_config::WasmConfig},
    storage::{
        global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_types::{bytesrepr::Bytes, ProtocolVersion, Timestamp};

use crate::{
    components::{fetcher::FetchResponse, Component, ComponentStatus},
    effect::{
        announcements::{ContractRuntimeAnnouncement, ControlAnnouncement},
        incoming::{TrieDemand, TrieRequest, TrieRequestIncoming},
        requests::{ContractRuntimeRequest, MarkBlockCompletedRequest, NetworkRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    types::{
        BlockHash, BlockHeader, Chainspec, ChainspecRawBytes, ChunkingError, Deploy,
        FinalizedBlock, TrieOrChunk, TrieOrChunkId,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use error::{BlockExecutionError, ConfigError};
use metrics::Metrics;
pub use operations::execute_finalized_block;
use operations::execute_only;
pub(crate) use types::{
    BlockAndExecutionEffects, EraValidatorsRequest, StepEffectAndUpcomingEraValidators,
};

/// An enum that represents all possible error conditions of a `contract_runtime` component.
#[derive(Debug, Error, From)]
pub(crate) enum ContractRuntimeError {
    /// The provided serialized id cannot be deserialized properly.
    #[error("error deserializing id: {0}")]
    InvalidSerializedId(#[source] bincode::Error),
    // It was not possible to get trie with the specified id
    #[error("error retrieving trie by id: {0}")]
    FailedToRetrieveTrieById(#[source] engine_state::Error),
    /// Chunking error.
    #[error("failed to chunk the data {0}")]
    ChunkingError(#[source] ChunkingError),
}

/// Maximum number of resource intensive tasks that can be run in parallel.
///
/// TODO: Fine tune this constant to the machine executing the node.
const MAX_PARALLEL_INTENSIVE_TASKS: usize = 4;

pub(crate) const APPROVALS_CHECKSUM_NAME: &str = "approvals_checksum";
pub(crate) const EXECUTION_RESULTS_CHECKSUM_NAME: &str = "execution_results_checksum";

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

#[derive(DataSize, Debug, Clone, Serialize)]
/// Wrapper for speculative execution prestate.
pub struct SpeculativeExecutionState {
    /// State root on top of which to execute deploy.
    pub state_root_hash: Digest,
    /// Block time.
    pub block_time: Timestamp,
    /// Protocol version used when creating the original block.
    pub protocol_version: ProtocolVersion,
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

    /// Creates instance of `ExecutionPreState` from given block header nad merkle tree hash
    /// activation point.
    pub fn from_block_header(block_header: &BlockHeader) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.hash(),
            parent_seed: block_header.accumulated_seed(),
        }
    }

    /// Returns the height of the next `Block` to be constructed. Note that this must match the
    /// height of the `FinalizedBlock` used to generate the block.
    pub(crate) fn next_block_height(&self) -> u64 {
        self.next_block_height
    }
}

type ExecQueue = Arc<Mutex<BTreeMap<u64, (FinalizedBlock, Vec<Deploy>, Vec<Deploy>)>>>;

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),

    #[from]
    TrieRequestIncoming(TrieRequestIncoming),

    #[from]
    TrieDemand(TrieDemand),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ContractRuntimeRequest(req) => {
                write!(f, "contract runtime request: {}", req)
            }
            Event::TrieRequestIncoming(req) => write!(f, "trie request incoming: {}", req),
            Event::TrieDemand(demand) => write!(f, "trie demand: {}", demand),
        }
    }
}

/// The contract runtime components.
#[derive(DataSize)]
pub(crate) struct ContractRuntime {
    status: ComponentStatus,
    execution_pre_state: Arc<Mutex<ExecutionPreState>>,
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<Metrics>,
    protocol_version: ProtocolVersion,

    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: ExecQueue,
    /// Cached instance of a [`SystemContractRegistry`].
    system_contract_registry: Option<SystemContractRegistry>,
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
        + From<NetworkRequest<Message>>
        + From<MarkBlockCompletedRequest>
        + Send,
{
    type Event = Event;
    type ConstructionError = ConfigError;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ContractRuntimeRequest(request) => {
                self.handle_contract_runtime_request(effect_builder, rng, request)
            }
            Event::TrieRequestIncoming(request) => {
                self.handle_trie_request(effect_builder, request)
            }
            Event::TrieDemand(demand) => self.handle_trie_demand(demand),
        }
    }
}

impl ContractRuntime {
    /// Handles an incoming request to get a trie.
    fn handle_trie_request<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        TrieRequestIncoming {
            sender,
            message: TrieRequest(ref serialized_id),
        }: TrieRequestIncoming,
    ) -> Effects<Event>
    where
        REv: From<NetworkRequest<Message>> + Send,
    {
        let fetch_response = match self.get_trie(serialized_id) {
            Ok(fetch_response) => fetch_response,
            Err(error) => {
                debug!("failed to get trie: {}", error);
                return Effects::new();
            }
        };

        match Message::new_get_response(&fetch_response) {
            Ok(message) => effect_builder.send_message(sender, message).ignore(),
            Err(error) => {
                error!("failed to create get-response: {}", error);
                Effects::new()
            }
        }
    }

    /// Handles an incoming demand for a trie.
    fn handle_trie_demand(
        &self,
        TrieDemand {
            request_msg: TrieRequest(ref serialized_id),
            auto_closing_responder,
            ..
        }: TrieDemand,
    ) -> Effects<Event> {
        let fetch_response = match self.get_trie(serialized_id) {
            Ok(fetch_response) => fetch_response,
            Err(error) => {
                // Something is wrong in our trie store, but be courteous and still send a reply.
                debug!("failed to get trie: {}", error);
                return auto_closing_responder.respond_none().ignore();
            }
        };

        match Message::new_get_response(&fetch_response) {
            Ok(message) => auto_closing_responder.respond(message).ignore(),
            Err(error) => {
                // This should never happen, but if it does, we let the peer know we cannot help.
                error!("failed to create get-response: {}", error);
                auto_closing_responder.respond_none().ignore()
            }
        }
    }

    /// Handles a contract runtime request.
    fn handle_contract_runtime_request<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        request: ContractRuntimeRequest,
    ) -> Effects<Event>
    where
        REv: From<ContractRuntimeRequest>
            + From<ContractRuntimeAnnouncement>
            + From<ControlAnnouncement>
            + From<MarkBlockCompletedRequest>
            + Send,
    {
        match request {
            ContractRuntimeRequest::CommitGenesis {
                chainspec,
                chainspec_raw_bytes,
                responder,
            } => {
                let result = self.commit_genesis(&chainspec, &chainspec_raw_bytes);
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
                let system_contract_registry = self.system_contract_registry.clone();
                let request = GetEraValidatorsRequest::new(state_root_hash, protocol_version);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let era_validators = engine_state.get_era_validators(
                        correlation_id,
                        system_contract_registry,
                        request,
                    );
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
                let system_contract_registry = self.system_contract_registry.clone();
                // Increment the counter to track the amount of times GetEraValidators was
                // requested.
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let era_validators = engine_state.get_era_validators(
                        correlation_id,
                        system_contract_registry,
                        request.into(),
                    );
                    metrics
                        .get_era_validators
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?era_validators, "get era validators response");
                    responder.respond(era_validators).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetTrie {
                trie_or_chunk_id,
                responder,
            } => {
                trace!(?trie_or_chunk_id, "get_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let result = Self::do_get_trie(&engine_state, &metrics, trie_or_chunk_id);
                    trace!(?result, "get_trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetTrieFull {
                trie_key,
                responder,
            } => {
                trace!(?trie_key, "get_trie_full request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let result = Self::get_trie_full(&engine_state, &metrics, trie_key);
                    trace!(?result, "get_trie_full response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::PutTrie {
                trie_bytes,
                responder,
            } => {
                trace!(?trie_bytes, "put_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.put_trie_and_find_missing_descendant_trie_keys(
                        correlation_id,
                        trie_bytes.inner(),
                    );
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
            ContractRuntimeRequest::FindMissingDescendantTrieKeys {
                trie_key,
                responder,
            } => {
                trace!(?trie_key, "find missing descendant trie keys");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let start = Instant::now();
                    let result = engine_state.missing_trie_keys(correlation_id, vec![trie_key]);
                    metrics
                        .missing_trie_keys
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "find missing descendant trie keys");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::SpeculativeDeployExecution {
                execution_prestate,
                deploy,
                responder,
            } => {
                let engine_state = Arc::clone(&self.engine_state);
                async move {
                    let result = run_intensive_task(move || {
                        execute_only(engine_state.as_ref(), execution_prestate, (*deploy).into())
                    })
                    .await;
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
        minimum_delegation_amount: u64,
        strict_argument_checking: bool,
        vesting_schedule_period_millis: u64,
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
            minimum_delegation_amount,
            strict_argument_checking,
            vesting_schedule_period_millis,
            wasm_config,
            system_config,
        );

        let engine_state = Arc::new(EngineState::new(global_state, engine_config));

        let metrics = Arc::new(Metrics::new(registry)?);

        Ok(ContractRuntime {
            status: ComponentStatus::Initialized,
            execution_pre_state,
            engine_state,
            metrics,
            protocol_version,
            exec_queue: Arc::new(Mutex::new(BTreeMap::new())),
            system_contract_registry: None,
        })
    }

    /// Commits a genesis request.
    fn commit_genesis(
        &self,
        chainspec: &Chainspec,
        chainspec_raw_bytes: &ChainspecRawBytes,
    ) -> Result<GenesisSuccess, engine_state::Error> {
        let correlation_id = CorrelationId::new();
        let genesis_config_hash = chainspec.hash();
        let protocol_version = chainspec.protocol_config.version;
        // Transforms a chainspec into a valid genesis config for execution engine.
        let ee_config = chainspec.into();

        let chainspec_registry = ChainspecRegistry::new_with_genesis(
            chainspec_raw_bytes.chainspec_bytes(),
            chainspec_raw_bytes
                .maybe_genesis_accounts_bytes()
                .ok_or_else(|| {
                    error!("failed to provide genesis account bytes in commit genesis");
                    engine_state::Error::Genesis(Box::new(
                        GenesisError::MissingChainspecRegistryEntry,
                    ))
                })?,
        );

        let result = self.engine_state.commit_genesis(
            correlation_id,
            genesis_config_hash,
            protocol_version,
            &ee_config,
            chainspec_registry,
        );
        self.engine_state.flush_environment()?;
        result
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
        self.engine_state.flush_environment()?;
        self.metrics
            .commit_upgrade
            .observe(start.elapsed().as_secs_f64());
        debug!(?result, "upgrade result");
        result
    }

    /// Retrieve trie keys
    #[cfg(test)]
    pub(crate) fn retrieve_trie_keys(
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

    pub(crate) fn set_initial_state(
        &mut self,
        sequential_block_state: ExecutionPreState,
    ) -> Result<(), ConfigError> {
        let mut execution_pre_state = self.execution_pre_state.lock().unwrap();
        *execution_pre_state = sequential_block_state;

        // Initialize the system contract registry.
        //
        // This is assumed to always work. In case following query fails we assume that the node is
        // incompatible with the network which could happen if (for example) a node operator skipped
        // important update and did not migrate old protocol data db into the global state.
        let state_root_hash = execution_pre_state.pre_state_root_hash;

        match self
            .engine_state
            .get_system_contract_registry(CorrelationId::default(), state_root_hash)
        {
            Ok(system_contract_registry) => {
                self.system_contract_registry = Some(system_contract_registry);
            }
            Err(error) => {
                error!(%state_root_hash, %error, "unable to initialize contract runtime with a system contract registry");
                return Err(error.into());
            }
        }

        Ok(())
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
            + From<MarkBlockCompletedRequest>
            + Send,
    {
        let current_execution_pre_state = execution_pre_state.lock().unwrap().clone();
        let BlockAndExecutionEffects {
            block,
            execution_results,
            maybe_step_effect_and_upcoming_era_validators,
            approvals_checksum,
            execution_results_checksum,
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

        let new_execution_pre_state = ExecutionPreState::from_block_header(block.header());
        *execution_pre_state.lock().unwrap() = new_execution_pre_state.clone();

        let current_era_id = block.header().era_id();

        let block_height = block.height();

        // Make sure storage is able to do its work before we declare success.
        effect_builder.mark_block_completed(block_height).await;

        if let Some(StepEffectAndUpcomingEraValidators {
            step_execution_journal,
            upcoming_era_validators,
        }) = maybe_step_effect_and_upcoming_era_validators
        {
            effect_builder
                .announce_commit_step_success(current_era_id, step_execution_journal)
                .await;

            effect_builder
                .announce_upcoming_era_validators(current_era_id, upcoming_era_validators)
                .await;
        }

        effect_builder
            .announce_new_linear_chain_block(
                block,
                approvals_checksum,
                execution_results_checksum,
                execution_results,
            )
            .await;

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

    /// Reads the trie (or chunk of a trie) under the given key and index.
    pub(crate) fn get_trie(
        &self,
        serialized_id: &[u8],
    ) -> Result<FetchResponse<TrieOrChunk, TrieOrChunkId>, ContractRuntimeError> {
        trace!(?serialized_id, "get_trie");

        let id: TrieOrChunkId = bincode::deserialize(serialized_id)?;
        let maybe_trie = Self::do_get_trie(&self.engine_state, &self.metrics, id)?;
        Ok(FetchResponse::from_opt(id, maybe_trie))
    }

    fn do_get_trie(
        engine_state: &EngineState<LmdbGlobalState>,
        metrics: &Metrics,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, ContractRuntimeError> {
        let correlation_id = CorrelationId::new();
        let start = Instant::now();
        let TrieOrChunkId(chunk_index, trie_key) = trie_or_chunk_id;
        let ret = match engine_state.get_trie_full(correlation_id, trie_key)? {
            None => Ok(None),
            Some(trie_raw) => Ok(Some(TrieOrChunk::new(trie_raw, chunk_index)?)),
        };
        metrics.get_trie.observe(start.elapsed().as_secs_f64());
        ret
    }

    fn get_trie_full(
        engine_state: &EngineState<LmdbGlobalState>,
        metrics: &Metrics,
        trie_key: Digest,
    ) -> Result<Option<Bytes>, engine_state::Error> {
        let correlation_id = CorrelationId::new();
        let start = Instant::now();
        let result = engine_state.get_trie_full(correlation_id, trie_key);
        metrics.get_trie.observe(start.elapsed().as_secs_f64());
        // Extract the inner Bytes, we don't want this change to ripple through the system right
        // now.
        result.map(|option| option.map(|trie_raw| trie_raw.into_inner()))
    }

    /// Returns the engine state, for testing only.
    #[cfg(test)]
    pub(crate) fn engine_state(&self) -> &Arc<EngineState<LmdbGlobalState>> {
        &self.engine_state
    }
}

#[cfg(test)]
mod tests {
    use casper_execution_engine::{
        shared::{
            additive_map::AdditiveMap, newtypes::CorrelationId, system_config::SystemConfig,
            transform::Transform, wasm_config::WasmConfig,
        },
        storage::trie::{Pointer, Trie},
    };
    use casper_hashing::{ChunkWithProof, Digest};
    use casper_types::{
        account::AccountHash, bytesrepr, CLValue, Key, ProtocolVersion, StoredValue,
    };
    use prometheus::Registry;
    use tempfile::tempdir;

    use crate::{
        components::fetcher::FetchResponse,
        contract_runtime::{Config as ContractRuntimeConfig, ContractRuntime},
        types::{ChunkingError, TrieOrChunk, TrieOrChunkId, ValueOrChunk},
    };

    use super::ContractRuntimeError;

    #[derive(Debug, Clone)]
    struct TestPair(Key, StoredValue);

    // Creates the test pairs that contain data of size
    // greater than the chunk limit.
    fn create_test_pairs_with_large_data() -> [TestPair; 2] {
        let val = CLValue::from_t(
            String::from_utf8(vec![b'a'; ChunkWithProof::CHUNK_SIZE_BYTES * 2]).unwrap(),
        )
        .unwrap();
        [
            TestPair(
                Key::Account(AccountHash::new([1_u8; 32])),
                StoredValue::CLValue(val.clone()),
            ),
            TestPair(
                Key::Account(AccountHash::new([2_u8; 32])),
                StoredValue::CLValue(val),
            ),
        ]
    }

    fn extract_next_hash_from_trie(trie_or_chunk: TrieOrChunk) -> Digest {
        let next_hash = if let TrieOrChunk::Value(trie_bytes) = trie_or_chunk {
            if let Trie::Node { pointer_block } =
                bytesrepr::deserialize::<Trie<Key, StoredValue>>(trie_bytes.into_inner().into())
                    .expect("Could not parse trie bytes")
            {
                if pointer_block.child_count() == 0 {
                    panic!("expected children");
                }
                let (_, ptr) = pointer_block.as_indexed_pointers().next().unwrap();
                match ptr {
                    Pointer::LeafPointer(ptr) | Pointer::NodePointer(ptr) => ptr,
                }
            } else {
                panic!("expected `Node`");
            }
        } else {
            panic!("expected `Trie`");
        };
        next_hash
    }

    // Creates a test ContractRuntime and feeds the underlying GlobalState with `test_pair`.
    // Returns [`ContractRuntime`] instance and the new merkle root after applying the `test_pair`.
    fn create_test_state(test_pair: [TestPair; 2]) -> (ContractRuntime, Digest) {
        let temp_dir = tempdir().unwrap();
        let contract_runtime = ContractRuntime::new(
            ProtocolVersion::default(),
            temp_dir.path(),
            &ContractRuntimeConfig::default(),
            WasmConfig::default(),
            SystemConfig::default(),
            10,
            10,
            10,
            true,
            1,
            &Registry::default(),
        )
        .unwrap();
        let empty_state_root = contract_runtime
            .engine_state()
            .get_state()
            .empty_state_root_hash();
        let mut effects: AdditiveMap<Key, Transform> = AdditiveMap::new();
        for TestPair(key, value) in test_pair {
            assert!(effects.insert(key, Transform::Write(value)).is_none());
        }
        let post_state_hash = contract_runtime
            .engine_state()
            .apply_effect(CorrelationId::new(), empty_state_root, effects)
            .expect("applying effects to succeed");
        (contract_runtime, post_state_hash)
    }

    fn read_trie(contract_runtime: &ContractRuntime, id: TrieOrChunkId) -> TrieOrChunk {
        let serialized_id = bincode::serialize(&id).unwrap();
        match contract_runtime
            .get_trie(&serialized_id)
            .expect("expected a successful read")
        {
            FetchResponse::Fetched(found) => found,
            FetchResponse::NotProvided(_) | FetchResponse::NotFound(_) => {
                panic!("expected to find the trie")
            }
        }
    }

    #[test]
    fn returns_trie_or_chunk() {
        let (contract_runtime, root_hash) = create_test_state(create_test_pairs_with_large_data());

        // Expect `Trie` with NodePointer when asking with a root hash.
        let trie = read_trie(&contract_runtime, TrieOrChunkId(0, root_hash));
        assert!(matches!(trie, ValueOrChunk::Value(_)));

        // Expect another `Trie` with two LeafPointers.
        let trie = read_trie(
            &contract_runtime,
            TrieOrChunkId(0, extract_next_hash_from_trie(trie)),
        );
        assert!(matches!(trie, TrieOrChunk::Value(_)));

        // Now, the next hash will point to the actual leaf, which as we expect
        // contains large data, so we expect to get `ChunkWithProof`.
        let hash = extract_next_hash_from_trie(trie);
        let chunk = match read_trie(&contract_runtime, TrieOrChunkId(0, hash)) {
            TrieOrChunk::ChunkWithProof(chunk) => chunk,
            other => panic!("expected ChunkWithProof, got {:?}", other),
        };

        assert_eq!(chunk.proof().root_hash(), hash);

        // try to read all the chunks
        let count = chunk.proof().count();
        let mut chunks = vec![chunk];
        for i in 1..count {
            let chunk = match read_trie(&contract_runtime, TrieOrChunkId(i, hash)) {
                TrieOrChunk::ChunkWithProof(chunk) => chunk,
                other => panic!("expected ChunkWithProof, got {:?}", other),
            };
            chunks.push(chunk);
        }

        // there should be no chunk with index `count`
        let serialized_id = bincode::serialize(&TrieOrChunkId(count, hash)).unwrap();
        assert!(matches!(
            contract_runtime.get_trie(&serialized_id),
            Err(ContractRuntimeError::ChunkingError(
                ChunkingError::MerkleConstruction(_)
            ))
        ));

        // all chunks should be valid
        assert!(chunks.iter().all(|chunk| chunk.verify().is_ok()));

        let data: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.into_chunk())
            .collect();

        let trie: Trie<Key, StoredValue> =
            bytesrepr::deserialize(data).expect("trie should deserialize correctly");

        // should be deserialized to a leaf
        assert!(matches!(trie, Trie::Leaf { .. }));
    }
}
