//! Contract Runtime component.

mod config;
mod error;
mod metrics;
mod operations;
mod rewards;
#[cfg(test)]
mod tests;
mod types;

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use lmdb::DatabaseFlags;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, info, trace};

#[cfg(test)]
use casper_execution_engine::engine_state::{GetBidsRequest, GetBidsResult};

use casper_execution_engine::engine_state::{
    self, genesis::GenesisError, DeployItem, EngineConfigBuilder, EngineState, GenesisSuccess,
    QueryRequest, SystemContractRegistry, UpgradeSuccess,
};
use casper_storage::{
    data_access_layer::{BlockStore, DataAccessLayer},
    global_state::{
        state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_types::{
    bytesrepr::Bytes, BlockHash, BlockHeaderV2, Chainspec, ChainspecRawBytes, ChainspecRegistry,
    Digest, EraId, ProtocolVersion, Timestamp, UpgradeConfig, U512,
};

use crate::{
    components::{fetcher::FetchResponse, Component, ComponentState},
    effect::{
        announcements::{
            ContractRuntimeAnnouncement, FatalAnnouncement, MetaBlockAnnouncement,
            UnexecutedBlockAnnouncement,
        },
        incoming::{TrieDemand, TrieRequest, TrieRequestIncoming},
        requests::{ContractRuntimeRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    types::{
        ChunkingError, ExecutableBlock, MetaBlock, MetaBlockState, TrieOrChunk, TrieOrChunkId,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use error::{BlockExecutionError, ConfigError};
use metrics::Metrics;
#[cfg(test)]
pub(crate) use operations::compute_execution_results_checksum;
pub use operations::execute_finalized_block;
use operations::execute_only;
pub(crate) use types::{
    BlockAndExecutionResults, EraValidatorsRequest, RoundSeigniorageRateRequest,
    StepEffectsAndUpcomingEraValidators, TotalSupplyRequest,
};

const COMPONENT_NAME: &str = "contract_runtime";

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

    /// Creates instance of `ExecutionPreState` from given block header nad Merkle tree hash
    /// activation point.
    pub fn from_block_header(block_header: &BlockHeaderV2) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.block_hash(),
            parent_seed: *block_header.accumulated_seed(),
        }
    }
}

use exec_queue::{ExecQueue, QueueItem};
mod exec_queue {
    use datasize::DataSize;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use crate::types::{ExecutableBlock, MetaBlockState};

    #[derive(Default, Clone, DataSize)]
    pub(super) struct ExecQueue(Arc<Mutex<BTreeMap<u64, QueueItem>>>);

    impl ExecQueue {
        /// How many blocks are backed up in the queue
        pub fn len(&self) -> usize {
            self.0
            .lock()
            .expect(
                "components::contract_runtime: couldn't get execution queue size; mutex poisoned",
            )
            .len()
        }

        pub fn remove(&mut self, height: u64) -> Option<QueueItem> {
            self.0
                .lock()
                .expect(
                    "components::contract_runtime: couldn't remove from the queue; mutex poisoned",
                )
                .remove(&height)
        }

        pub fn insert(&mut self, height: u64, item: QueueItem) {
            self.0
                .lock()
                .expect(
                    "components::contract_runtime: couldn't insert into the queue; mutex poisoned",
                )
                .insert(height, item);
        }

        /// Remove every entry older than the given height, and return the new len.
        pub fn remove_older_then(&mut self, height: u64) -> i64 {
            let mut locked_queue = self.0
            .lock()
            .expect(
                "components::contract_runtime: couldn't initialize contract runtime block execution queue; mutex poisoned"
            );

            *locked_queue = locked_queue.split_off(&height);

            core::convert::TryInto::try_into(locked_queue.len()).unwrap_or(i64::MIN)
        }
    }

    // Should it be an enum?
    pub(super) struct QueueItem {
        pub executable_block: ExecutableBlock,
        pub meta_block_state: MetaBlockState,
    }
}

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
    state: ComponentState,
    execution_pre_state: Arc<Mutex<ExecutionPreState>>,
    engine_state: Arc<EngineState<DataAccessLayer<LmdbGlobalState>>>,
    metrics: Arc<Metrics>,

    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: ExecQueue,
    /// Cached instance of a [`SystemContractRegistry`].
    system_contract_registry: Option<SystemContractRegistry>,
    chainspec: Arc<Chainspec>,
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
        + From<NetworkRequest<Message>>
        + From<StorageRequest>
        + From<MetaBlockAnnouncement>
        + From<UnexecutedBlockAnnouncement>
        + From<FatalAnnouncement>
        + Send,
{
    type Event = Event;

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

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl ContractRuntime {
    /// How many blocks are backed up in the queue
    pub(crate) fn queue_depth(&self) -> usize {
        self.exec_queue.len()
    }

    /// Handles an incoming request to get a trie.
    fn handle_trie_request<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        TrieRequestIncoming { sender, message }: TrieRequestIncoming,
    ) -> Effects<Event>
    where
        REv: From<NetworkRequest<Message>> + Send,
    {
        let TrieRequest(ref serialized_id) = *message;
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
            request_msg,
            auto_closing_responder,
            ..
        }: TrieDemand,
    ) -> Effects<Event> {
        let TrieRequest(ref serialized_id) = *request_msg;
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
            + From<StorageRequest>
            + From<MetaBlockAnnouncement>
            + From<UnexecutedBlockAnnouncement>
            + From<FatalAnnouncement>
            + Send,
    {
        match request {
            ContractRuntimeRequest::Query {
                query_request,
                responder,
            } => {
                trace!(?query_request, "query");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let start = Instant::now();
                    let result = engine_state.run_query(query_request);
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
                    let start = Instant::now();
                    let result = engine_state.get_purse_balance(
                        balance_request.state_hash(),
                        balance_request.purse_uref(),
                    );
                    metrics.get_balance.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "balance result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetTotalSupply {
                total_supply_request,
                responder,
            } => {
                trace!(?total_supply_request, "get total supply request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);

                async move {
                    let start = Instant::now();
                    let result = query_total_supply(engine_state, total_supply_request.state_hash);

                    metrics
                        .get_total_supply
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "total supply result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetRoundSeigniorageRate {
                round_seigniorage_rate_request,
                responder,
            } => {
                trace!(
                    ?round_seigniorage_rate_request,
                    "get round seigniorage rate request"
                );
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);

                async move {
                    let start = Instant::now();
                    let result = query_round_seigniorage_rate(
                        engine_state,
                        round_seigniorage_rate_request.state_hash,
                    );

                    metrics
                        .get_round_seigniorage_rate
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "round seigniorage rate result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetEraValidators { request, responder } => {
                trace!(?request, "get era validators request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);

                self.try_init_system_contract_registry_cache();

                let system_contract_registry = self.system_contract_registry.clone();
                // Increment the counter to track the amount of times GetEraValidators was
                // requested.
                async move {
                    let start = Instant::now();
                    let era_validators =
                        engine_state.get_era_validators(system_contract_registry, request.into());
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
                    let start = Instant::now();
                    let result = engine_state.put_trie_if_all_children_present(trie_bytes.inner());
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
            ContractRuntimeRequest::EnqueueBlockForExecution {
                executable_block,
                key_block_height_for_activation_point,
                meta_block_state,
            } => {
                let mut effects = Effects::new();
                let mut exec_queue = self.exec_queue.clone();
                let finalized_block_height = executable_block.height;
                let current_pre_state = self.execution_pre_state.lock().unwrap();
                let next_block_height = current_pre_state.next_block_height;
                match finalized_block_height.cmp(&next_block_height) {
                    // An old block: it won't be executed:
                    Ordering::Less => {
                        debug!(
                            "ContractRuntime: finalized block({}) precedes expected next block({})",
                            finalized_block_height, next_block_height
                        );
                        effects.extend(
                            effect_builder
                                .announce_unexecuted_block(finalized_block_height)
                                .ignore(),
                        );
                    }
                    // This is the next block to be executed, we do it right away:
                    Ordering::Equal => {
                        info!(
                            "ContractRuntime: execute finalized block({}) with {} deploys",
                            finalized_block_height,
                            executable_block.deploys.len()
                        );
                        let engine_state = Arc::clone(&self.engine_state);
                        let metrics = Arc::clone(&self.metrics);
                        let shared_pre_state = Arc::clone(&self.execution_pre_state);
                        effects.extend(
                            Self::execute_executable_block_or_requeue(
                                engine_state,
                                metrics,
                                self.chainspec.clone(),
                                exec_queue,
                                shared_pre_state,
                                current_pre_state.clone(),
                                effect_builder,
                                executable_block,
                                key_block_height_for_activation_point,
                                meta_block_state,
                            )
                            .ignore(),
                        )
                    }
                    // This is a future block, we store it into exec_queue, to be executed later:
                    Ordering::Greater => {
                        debug!(
                            "ContractRuntime: enqueuing({}) waiting for({})",
                            finalized_block_height, next_block_height
                        );
                        info!(
                            "ContractRuntime: enqueuing finalized block({}) with {} deploys for execution",
                            finalized_block_height,
                            executable_block.    deploys.len()
                        );
                        exec_queue.insert(
                            finalized_block_height,
                            QueueItem {
                                executable_block,
                                meta_block_state,
                            },
                        );
                    }
                }
                self.metrics
                    .exec_queue_size
                    .set(self.exec_queue.len().try_into().unwrap_or(i64::MIN));
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
                    let start = Instant::now();
                    let result = engine_state.get_bids(get_bids_request);
                    metrics.get_bids.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get bids result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            } => {
                let result = self
                    .engine_state
                    .get_checksum_registry(state_root_hash)
                    .map(|maybe_registry| {
                        maybe_registry.and_then(|registry| {
                            registry.get(EXECUTION_RESULTS_CHECKSUM_NAME).copied()
                        })
                    });
                responder.respond(result).ignore()
            }
            ContractRuntimeRequest::GetAddressableEntity {
                state_root_hash,
                key,
                responder,
            } => {
                let engine_state = Arc::clone(&self.engine_state);
                async move {
                    let result =
                        operations::get_addressable_entity(&engine_state, state_root_hash, key);
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
                        execute_only(
                            engine_state.as_ref(),
                            execution_prestate,
                            DeployItem::from((*deploy).clone()),
                        )
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
    pub(crate) fn new(
        storage_dir: &Path,
        contract_runtime_config: &Config,
        chainspec: Arc<Chainspec>,
        registry: &Registry,
    ) -> Result<Self, ConfigError> {
        // TODO: This is bogus, get rid of this
        let execution_pre_state = Arc::new(Mutex::new(ExecutionPreState {
            pre_state_root_hash: Default::default(),
            next_block_height: 0,
            parent_hash: Default::default(),
            parent_seed: Default::default(),
        }));

        let data_access_layer = {
            let environment = Arc::new(LmdbEnvironment::new(
                storage_dir,
                contract_runtime_config.max_global_state_size_or_default(),
                contract_runtime_config.max_readers_or_default(),
                contract_runtime_config.manual_sync_enabled_or_default(),
            )?);

            let trie_store = Arc::new(LmdbTrieStore::new(
                &environment,
                None,
                DatabaseFlags::empty(),
            )?);

            let global_state = LmdbGlobalState::empty(environment, trie_store)?;

            let block_store = BlockStore::new();

            DataAccessLayer {
                state: global_state,
                block_store,
            }
        };

        let engine_config = EngineConfigBuilder::new()
            .with_max_query_depth(contract_runtime_config.max_query_depth_or_default())
            .with_max_associated_keys(chainspec.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(chainspec.core_config.max_runtime_call_stack_height)
            .with_minimum_delegation_amount(chainspec.core_config.minimum_delegation_amount)
            .with_strict_argument_checking(chainspec.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(
                chainspec.core_config.vesting_schedule_period.millis(),
            )
            .with_max_delegators_per_validator(
                (chainspec.core_config.max_delegators_per_validator != 0)
                    .then_some(chainspec.core_config.max_delegators_per_validator),
            )
            .with_wasm_config(chainspec.wasm_config)
            .with_system_config(chainspec.system_costs_config)
            .with_administrative_accounts(chainspec.core_config.administrators.clone())
            .with_allow_auction_bids(chainspec.core_config.allow_auction_bids)
            .with_allow_unrestricted_transfers(chainspec.core_config.allow_unrestricted_transfers)
            .with_refund_handling(chainspec.core_config.refund_handling)
            .with_fee_handling(chainspec.core_config.fee_handling)
            .build();

        let engine_state = EngineState::new(data_access_layer, engine_config);

        let engine_state = Arc::new(engine_state);

        let metrics = Arc::new(Metrics::new(registry)?);

        Ok(ContractRuntime {
            state: ComponentState::Initialized,
            execution_pre_state,
            engine_state,
            metrics,
            exec_queue: Default::default(),
            system_contract_registry: None,
            chainspec,
        })
    }

    /// Commits a genesis request.
    pub(crate) fn commit_genesis(
        &self,
        chainspec: &Chainspec,
        chainspec_raw_bytes: &ChainspecRawBytes,
    ) -> Result<GenesisSuccess, engine_state::Error> {
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
            genesis_config_hash,
            protocol_version,
            &ee_config,
            chainspec_registry,
        );
        self.engine_state.flush_environment()?;
        result
    }

    pub(crate) fn commit_upgrade(
        &self,
        upgrade_config: UpgradeConfig,
    ) -> Result<UpgradeSuccess, engine_state::Error> {
        debug!(?upgrade_config, "upgrade");
        let start = Instant::now();
        let scratch_state = self.engine_state.get_scratch_engine_state();
        let pre_state_hash = upgrade_config.pre_state_hash();
        let mut result = scratch_state.commit_upgrade(upgrade_config)?;
        result.post_state_hash = self
            .engine_state
            .write_scratch_to_db(pre_state_hash, scratch_state.into_inner())?;
        self.engine_state.flush_environment()?;
        self.metrics
            .commit_upgrade
            .observe(start.elapsed().as_secs_f64());
        debug!(?result, "upgrade result");
        Ok(result)
    }

    pub(crate) fn set_initial_state(&mut self, sequential_block_state: ExecutionPreState) {
        let next_block_height = sequential_block_state.next_block_height;
        let mut execution_pre_state = self.execution_pre_state.lock().unwrap();
        *execution_pre_state = sequential_block_state;

        let new_len = self
            .exec_queue
            .remove_older_then(execution_pre_state.next_block_height);
        self.metrics.exec_queue_size.set(new_len);
        debug!(next_block_height, "ContractRuntime: set initial state");
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_executable_block_or_requeue<REv>(
        engine_state: Arc<EngineState<DataAccessLayer<LmdbGlobalState>>>,
        metrics: Arc<Metrics>,
        chainspec: Arc<Chainspec>,
        mut exec_queue: ExecQueue,
        shared_pre_state: Arc<Mutex<ExecutionPreState>>,
        current_pre_state: ExecutionPreState,
        effect_builder: EffectBuilder<REv>,
        mut executable_block: ExecutableBlock,
        key_block_height_for_activation_point: u64,
        mut meta_block_state: MetaBlockState,
    ) where
        REv: From<ContractRuntimeRequest>
            + From<ContractRuntimeAnnouncement>
            + From<StorageRequest>
            + From<MetaBlockAnnouncement>
            + From<FatalAnnouncement>
            + Send,
    {
        debug!("ContractRuntime: execute_finalized_block_or_requeue");
        let contract_runtime_metrics = metrics.clone();

        let protocol_version = chainspec.protocol_version();
        let activation_point = chainspec.protocol_config.activation_point;
        let prune_batch_size = chainspec.core_config.prune_batch_size;
        if executable_block.era_report.is_some() && executable_block.rewards.is_none() {
            executable_block.rewards = Some(if chainspec.core_config.compute_rewards {
                match rewards::rewards_for_era(
                    effect_builder,
                    executable_block.era_id,
                    executable_block.height,
                    chainspec,
                )
                .await
                {
                    Ok(rewards) => rewards,
                    Err(e) => {
                        return fatal!(effect_builder, "Failed to compute the rewards: {e:?}").await
                    }
                }
            } else {
                //TODO instead, use a list of all the validators with 0
                BTreeMap::new()
            });
        }

        let BlockAndExecutionResults {
            block,
            approvals_hashes,
            execution_results,
            maybe_step_effects_and_upcoming_era_validators,
        } = match run_intensive_task(move || {
            debug!("ContractRuntime: execute_finalized_block");
            execute_finalized_block(
                engine_state.as_ref(),
                Some(contract_runtime_metrics),
                protocol_version,
                current_pre_state,
                executable_block,
                activation_point.era_id(),
                key_block_height_for_activation_point,
                prune_batch_size,
            )
        })
        .await
        {
            Ok(block_and_execution_results) => block_and_execution_results,
            Err(error) => {
                error!(%error, "failed to execute block");
                return fatal!(effect_builder, "{}", error).await;
            }
        };

        let new_execution_pre_state = ExecutionPreState::from_block_header(block.header());
        {
            // The `shared_pre_state` could have been set to a block we just fully synced after
            // doing a sync leap (via a call to `set_initial_state`).  We should not allow a block
            // which completed execution just after this to set the `shared_pre_state` back to an
            // earlier block height.
            let mut shared_pre_state = shared_pre_state.lock().unwrap();
            if shared_pre_state.next_block_height < new_execution_pre_state.next_block_height {
                debug!(
                    next_block_height = new_execution_pre_state.next_block_height,
                    "ContractRuntime: updating shared pre-state",
                );
                *shared_pre_state = new_execution_pre_state.clone();
            } else {
                debug!(
                    current_next_block_height = shared_pre_state.next_block_height,
                    attempted_next_block_height = new_execution_pre_state.next_block_height,
                    "ContractRuntime: not updating shared pre-state to older state"
                );
            }
        }

        let current_era_id = block.era_id();

        if let Some(StepEffectsAndUpcomingEraValidators {
            step_effects,
            mut upcoming_era_validators,
        }) = maybe_step_effects_and_upcoming_era_validators
        {
            effect_builder
                .announce_commit_step_success(current_era_id, step_effects)
                .await;

            if current_era_id.is_genesis() {
                match upcoming_era_validators
                    .get(&current_era_id.successor())
                    .cloned()
                {
                    Some(era_validators) => {
                        upcoming_era_validators.insert(EraId::default(), era_validators);
                    }
                    None => {
                        fatal!(effect_builder, "Missing era 1 validators").await;
                    }
                }
            }

            effect_builder
                .announce_upcoming_era_validators(current_era_id, upcoming_era_validators)
                .await;
        }

        info!(
            block_hash = %block.hash(),
            height = block.height(),
            era = block.era_id().value(),
            is_switch_block = block.is_switch_block(),
            "executed block"
        );

        let execution_results_map: HashMap<_, _> = execution_results
            .iter()
            .cloned()
            .map(|(deploy_hash, _, execution_result)| (deploy_hash, execution_result))
            .collect();
        if meta_block_state.register_as_stored().was_updated() {
            effect_builder
                .put_executed_block_to_storage(
                    Arc::clone(&block),
                    approvals_hashes,
                    execution_results_map,
                )
                .await;
        } else {
            effect_builder
                .put_approvals_hashes_to_storage(approvals_hashes)
                .await;
            effect_builder
                .put_execution_results_to_storage(
                    *block.hash(),
                    block.height(),
                    execution_results_map,
                )
                .await;
        }
        if meta_block_state
            .register_as_executed()
            .was_already_registered()
        {
            error!(
                block_hash = %block.hash(),
                block_height = block.height(),
                ?meta_block_state,
                "should not execute the same block more than once"
            );
        }

        let meta_block = MetaBlock::new_forward(block, execution_results, meta_block_state);
        effect_builder.announce_meta_block(meta_block).await;

        // If the child is already finalized, start execution.
        let next_block = exec_queue.remove(new_execution_pre_state.next_block_height);

        // We schedule the next block from the queue to be executed:
        if let Some(QueueItem {
            executable_block,
            meta_block_state,
        }) = next_block
        {
            metrics.exec_queue_size.dec();
            debug!("ContractRuntime: next block enqueue_block_for_execution");
            effect_builder
                .enqueue_block_for_execution(executable_block, meta_block_state)
                .await;
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
        engine_state: &EngineState<DataAccessLayer<LmdbGlobalState>>,
        metrics: &Metrics,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, ContractRuntimeError> {
        let start = Instant::now();
        let TrieOrChunkId(chunk_index, trie_key) = trie_or_chunk_id;
        let ret = match engine_state.get_trie_full(trie_key)? {
            None => Ok(None),
            Some(trie_raw) => Ok(Some(TrieOrChunk::new(trie_raw.into(), chunk_index)?)),
        };
        metrics.get_trie.observe(start.elapsed().as_secs_f64());
        ret
    }

    fn get_trie_full(
        engine_state: &EngineState<DataAccessLayer<LmdbGlobalState>>,
        metrics: &Metrics,
        trie_key: Digest,
    ) -> Result<Option<Bytes>, engine_state::Error> {
        let start = Instant::now();
        let result = engine_state.get_trie_full(trie_key);
        metrics.get_trie.observe(start.elapsed().as_secs_f64());
        // Extract the inner Bytes, we don't want this change to ripple through the system right
        // now.
        result.map(|option| option.map(|trie_raw| trie_raw.into_inner()))
    }

    #[inline]
    fn try_init_system_contract_registry_cache(&mut self) {
        // The system contract registry is stable so we can use the latest state root hash that we
        // know from the execution pre-state to try and initialize it
        let state_root_hash = self
            .execution_pre_state
            .lock()
            .expect("ContractRuntime: execution_pre_state poisoned mutex")
            .pre_state_root_hash;

        // Try to cache system contract registry if possible.
        if self.system_contract_registry.is_none() {
            self.system_contract_registry = self
                .engine_state
                .get_system_contract_registry(state_root_hash)
                .ok();
        };
    }

    /// Returns the engine state, for testing only.
    #[cfg(test)]
    pub(crate) fn engine_state(&self) -> &Arc<EngineState<DataAccessLayer<LmdbGlobalState>>> {
        &self.engine_state
    }

    /// Returns auction state, for testing only.
    #[cfg(test)]
    pub(crate) fn auction_state(
        &self,
        root_hash: Digest,
    ) -> Result<GetBidsResult, engine_state::Error> {
        let engine_state = Arc::clone(&self.engine_state);
        let get_bids_request = GetBidsRequest::new(root_hash);
        engine_state.get_bids(get_bids_request)
    }
}

fn query_total_supply(
    engine_state: Arc<EngineState<DataAccessLayer<LmdbGlobalState>>>,
    state_hash: Digest,
) -> Result<U512, engine_state::Error> {
    use casper_types::system::mint;
    use engine_state::{Error, QueryResult::*};

    let mint = engine_state.get_system_mint_hash(state_hash)?;
    let request = QueryRequest::new(
        state_hash,
        mint.into(),
        vec![mint::TOTAL_SUPPLY_KEY.to_owned()],
    );

    engine_state
        .run_query(request)
        .and_then(move |query_result| match query_result {
            Success { value, proofs: _ } => value
                .as_cl_value()
                .ok_or_else(|| Error::Mint("Value not a CLValue".to_owned()))?
                .clone()
                .into_t()
                .map_err(|e| Error::Mint(format!("CLValue not a U512: {e}"))),
            ValueNotFound(s) => Err(Error::Mint(format!("ValueNotFound({s})"))),
            CircularReference(s) => Err(Error::Mint(format!("CircularReference({s})"))),
            DepthLimit { depth } => Err(Error::Mint(format!("DepthLimit({depth})"))),
            RootNotFound => Err(Error::RootNotFound(state_hash)),
        })
}

fn query_round_seigniorage_rate(
    engine_state: Arc<EngineState<DataAccessLayer<LmdbGlobalState>>>,
    state_hash: Digest,
) -> Result<Ratio<U512>, engine_state::Error> {
    use casper_types::system::mint;
    use engine_state::{Error, QueryResult::*};

    let mint = engine_state.get_system_mint_hash(state_hash)?;
    let request = QueryRequest::new(
        state_hash,
        mint.into(),
        vec![mint::ROUND_SEIGNIORAGE_RATE_KEY.to_owned()],
    );

    engine_state
        .run_query(request)
        .and_then(move |query_result| match query_result {
            Success { value, proofs: _ } => value
                .as_cl_value()
                .ok_or_else(|| Error::Mint("Value not a CLValue".to_owned()))?
                .clone()
                .into_t()
                .map_err(|e| Error::Mint(format!("CLValue not a Ratio<U512>: {e}"))),
            ValueNotFound(s) => Err(Error::Mint(format!("ValueNotFound({s})"))),
            CircularReference(s) => Err(Error::Mint(format!("CircularReference({s})"))),
            DepthLimit { depth } => Err(Error::Mint(format!("DepthLimit({depth})"))),
            RootNotFound => Err(Error::RootNotFound(state_hash)),
        })
}

#[cfg(test)]
mod trie_chunking_tests {
    use std::sync::Arc;

    use casper_execution_engine::engine_state::engine_config::{
        DEFAULT_FEE_HANDLING, DEFAULT_REFUND_HANDLING,
    };
    use casper_storage::global_state::{
        state::StateProvider,
        trie::{Pointer, Trie},
    };
    use casper_types::{
        account::AccountHash,
        bytesrepr,
        execution::{Transform, TransformKind},
        testing::TestRng,
        ActivationPoint, CLValue, Chainspec, ChunkWithProof, CoreConfig, Digest, EraId, Key,
        ProtocolConfig, StoredValue, TimeDiff,
    };
    use prometheus::Registry;
    use tempfile::tempdir;

    use crate::{
        components::fetcher::FetchResponse,
        contract_runtime::ContractRuntimeError,
        types::{ChunkingError, TrieOrChunk, TrieOrChunkId, ValueOrChunk},
    };

    use super::{Config as ContractRuntimeConfig, ContractRuntime};

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
            if let Trie::Node { pointer_block } = bytesrepr::deserialize::<Trie<Key, StoredValue>>(
                trie_bytes.into_inner().into_inner().into(),
            )
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
    // Returns [`ContractRuntime`] instance and the new Merkle root after applying the `test_pair`.
    fn create_test_state(rng: &mut TestRng, test_pair: [TestPair; 2]) -> (ContractRuntime, Digest) {
        let temp_dir = tempdir().unwrap();
        let chainspec = Chainspec {
            protocol_config: ProtocolConfig {
                activation_point: ActivationPoint::EraId(EraId::from(2)),
                ..ProtocolConfig::random(rng)
            },
            core_config: CoreConfig {
                max_associated_keys: 10,
                max_runtime_call_stack_height: 10,
                minimum_delegation_amount: 10,
                prune_batch_size: 5,
                strict_argument_checking: true,
                vesting_schedule_period: TimeDiff::from_millis(1),
                max_delegators_per_validator: 0,
                allow_auction_bids: true,
                allow_unrestricted_transfers: true,
                fee_handling: DEFAULT_FEE_HANDLING,
                refund_handling: DEFAULT_REFUND_HANDLING,
                ..CoreConfig::random(rng)
            },
            wasm_config: Default::default(),
            system_costs_config: Default::default(),
            ..Chainspec::random(rng)
        };
        let contract_runtime = ContractRuntime::new(
            temp_dir.path(),
            &ContractRuntimeConfig::default(),
            Arc::new(chainspec),
            &Registry::default(),
        )
        .unwrap();
        let empty_state_root = contract_runtime.engine_state().get_state().empty_root();
        let mut effects = casper_types::execution::Effects::new();
        for TestPair(key, value) in test_pair {
            effects.push(Transform::new(key, TransformKind::Write(value)));
        }
        let post_state_hash = contract_runtime
            .engine_state()
            .commit_effects(empty_state_root, effects)
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
        let rng = &mut TestRng::new();
        let (contract_runtime, root_hash) =
            create_test_state(rng, create_test_pairs_with_large_data());

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
