//! Contract Runtime component.

mod config;
mod error;
mod event;
mod exec_queue;
mod metrics;
mod operations;
mod rewards;
#[cfg(test)]
mod tests;
mod types;
mod utils;

use std::{
    cmp::Ordering,
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use datasize::DataSize;
use lmdb::DatabaseFlags;
use prometheus::Registry;
use tracing::{debug, error, info, trace};

use casper_execution_engine::engine_state::{EngineConfigBuilder, ExecutionEngineV1};
use casper_storage::{
    data_access_layer::{
        AddressableEntityRequest, AddressableEntityResult, BlockStore, DataAccessLayer,
        EntryPointsRequest, ExecutionResultsChecksumRequest, FlushRequest, FlushResult,
        GenesisRequest, GenesisResult, TrieRequest,
    },
    global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
    system::genesis::GenesisError,
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::AccountHash, ActivationPoint, Chainspec, ChainspecRawBytes, ChainspecRegistry,
    EntityAddr, EraId, Key, PublicKey,
};

use crate::{
    components::{fetcher::FetchResponse, Component, ComponentState},
    contract_runtime::{types::EraPrice, utils::handle_protocol_upgrade},
    effect::{
        announcements::{
            ContractRuntimeAnnouncement, FatalAnnouncement, MetaBlockAnnouncement,
            UnexecutedBlockAnnouncement,
        },
        incoming::{TrieDemand, TrieRequest as TrieRequestMessage, TrieRequestIncoming},
        requests::{ContractRuntimeRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    types::{
        BlockPayload, ExecutableBlock, FinalizedBlock, InternalEraReport, MetaBlockState,
        TrieOrChunk, TrieOrChunkId,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use error::{BlockExecutionError, ConfigError, ContractRuntimeError};
pub(crate) use event::Event;
use exec_queue::{ExecQueue, QueueItem};
use metrics::Metrics;
#[cfg(test)]
pub(crate) use operations::compute_execution_results_checksum;
pub use operations::execute_finalized_block;
use operations::speculatively_execute;
pub(crate) use types::{
    BlockAndExecutionArtifacts, ExecutionArtifact, ExecutionPreState, SpeculativeExecutionResult,
    StepOutcome,
};
use utils::{exec_or_requeue, run_intensive_task};

const COMPONENT_NAME: &str = "contract_runtime";

pub(crate) const APPROVALS_CHECKSUM_NAME: &str = "approvals_checksum";
pub(crate) const EXECUTION_RESULTS_CHECKSUM_NAME: &str = "execution_results_checksum";

/// The contract runtime components.
#[derive(DataSize)]
pub(crate) struct ContractRuntime {
    state: ComponentState,
    execution_pre_state: Arc<Mutex<ExecutionPreState>>,
    #[data_size(skip)]
    execution_engine_v1: Arc<ExecutionEngineV1>,
    metrics: Arc<Metrics>,
    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: ExecQueue,
    /// The chainspec.
    chainspec: Arc<Chainspec>,
    #[data_size(skip)]
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    current_gas_price: EraPrice,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContractRuntime").finish()
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
        let execution_pre_state = Arc::new(Mutex::new(ExecutionPreState::default()));

        let current_gas_price = match chainspec.protocol_config.activation_point {
            ActivationPoint::EraId(era_id) => {
                EraPrice::new(era_id, chainspec.vacancy_config.min_gas_price)
            }
            ActivationPoint::Genesis(_) => {
                EraPrice::new(EraId::new(0), chainspec.vacancy_config.min_gas_price)
            }
        };

        let engine_config = EngineConfigBuilder::new()
            .with_max_query_depth(contract_runtime_config.max_query_depth_or_default())
            .with_max_associated_keys(chainspec.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(chainspec.core_config.max_runtime_call_stack_height)
            .with_minimum_delegation_amount(chainspec.core_config.minimum_delegation_amount)
            .with_maximum_delegation_amount(chainspec.core_config.maximum_delegation_amount)
            .with_strict_argument_checking(chainspec.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(
                chainspec.core_config.vesting_schedule_period.millis(),
            )
            .with_max_delegators_per_validator(chainspec.core_config.max_delegators_per_validator)
            .with_wasm_config(chainspec.wasm_config)
            .with_system_config(chainspec.system_costs_config)
            .with_administrative_accounts(chainspec.core_config.administrators.clone())
            .with_allow_auction_bids(chainspec.core_config.allow_auction_bids)
            .with_allow_unrestricted_transfers(chainspec.core_config.allow_unrestricted_transfers)
            .with_refund_handling(chainspec.core_config.refund_handling)
            .with_fee_handling(chainspec.core_config.fee_handling)
            .with_protocol_version(chainspec.protocol_version())
            .build();

        let data_access_layer = Arc::new(
            Self::new_data_access_layer(storage_dir, contract_runtime_config)
                .map_err(ConfigError::GlobalState)?,
        );

        let execution_engine_v1 = Arc::new(ExecutionEngineV1::new(engine_config));

        let metrics = Arc::new(Metrics::new(registry)?);

        Ok(ContractRuntime {
            state: ComponentState::Initialized,
            execution_pre_state,
            execution_engine_v1,
            metrics,
            exec_queue: Default::default(),
            chainspec,
            data_access_layer,
            current_gas_price,
        })
    }

    pub(crate) fn set_initial_state(&mut self, sequential_block_state: ExecutionPreState) {
        let next_block_height = sequential_block_state.next_block_height();
        let mut execution_pre_state = self.execution_pre_state.lock().unwrap();
        *execution_pre_state = sequential_block_state;

        let new_len = self
            .exec_queue
            .remove_older_than(execution_pre_state.next_block_height());
        self.metrics.exec_queue_size.set(new_len);
        debug!(next_block_height, "ContractRuntime: set initial state");
    }

    fn new_data_access_layer(
        storage_dir: &Path,
        contract_runtime_config: &Config,
    ) -> Result<DataAccessLayer<LmdbGlobalState>, casper_storage::global_state::error::Error> {
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

            let block_store = BlockStore::new();

            let max_query_depth = contract_runtime_config.max_query_depth_or_default();
            let global_state = LmdbGlobalState::empty(environment, trie_store, max_query_depth)?;

            DataAccessLayer {
                state: global_state,
                block_store,
                max_query_depth,
            }
        };
        Ok(data_access_layer)
    }

    /// How many blocks are backed up in the queue
    pub(crate) fn queue_depth(&self) -> usize {
        self.exec_queue.len()
    }

    /// Commits a genesis request.
    pub(crate) fn commit_genesis(
        &self,
        chainspec: &Chainspec,
        chainspec_raw_bytes: &ChainspecRawBytes,
    ) -> GenesisResult {
        debug!("commit_genesis");
        let start = Instant::now();
        let protocol_version = chainspec.protocol_config.version;
        let chainspec_hash = chainspec.hash();
        let genesis_config = chainspec.into();
        let account_bytes = match chainspec_raw_bytes.maybe_genesis_accounts_bytes() {
            Some(bytes) => bytes,
            None => {
                error!("failed to provide genesis account bytes in commit genesis");
                return GenesisResult::Failure(GenesisError::MissingGenesisAccounts);
            }
        };

        let chainspec_registry = ChainspecRegistry::new_with_genesis(
            chainspec_raw_bytes.chainspec_bytes(),
            account_bytes,
        );

        let genesis_request = GenesisRequest::new(
            chainspec_hash,
            protocol_version,
            genesis_config,
            chainspec_registry,
        );

        let data_access_layer = Arc::clone(&self.data_access_layer);
        let result = data_access_layer.genesis(genesis_request);
        self.metrics
            .commit_genesis
            .observe(start.elapsed().as_secs_f64());
        debug!(?result, "upgrade result");
        if result.is_success() {
            let flush_req = FlushRequest::new();
            if let FlushResult::Failure(err) = data_access_layer.flush(flush_req) {
                return GenesisResult::Failure(GenesisError::TrackingCopy(
                    TrackingCopyError::Storage(err),
                ));
            }
        }
        result
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
                request: query_request,
                responder,
            } => {
                trace!(?query_request, "query");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.query(query_request);
                    metrics.run_query.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "query result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::QueryByPrefix {
                request: query_request,
                responder,
            } => {
                trace!(?query_request, "query by prefix");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();

                    let result = data_access_layer.prefixed_values(query_request);
                    metrics.run_query.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "query by prefix result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetBalance {
                request: balance_request,
                responder,
            } => {
                trace!(?balance_request, "balance");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.balance(balance_request);
                    metrics.get_balance.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "balance result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetEraValidators {
                request: era_validators_request,
                responder,
            } => {
                trace!(?era_validators_request, "get era validators request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.era_validators(era_validators_request);
                    metrics
                        .get_era_validators
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "era validators result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetSeigniorageRecipients { request, responder } => {
                trace!(?request, "get seigniorage recipients request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.seigniorage_recipients(request);
                    metrics
                        .get_seigniorage_recipients
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "seigniorage recipients result");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            } => {
                trace!(?state_root_hash, "get execution results checksum request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let request = ExecutionResultsChecksumRequest::new(state_root_hash);
                    let result = data_access_layer.execution_result_checksum(request);
                    metrics
                        .execution_results_checksum
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "execution result checksum");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetAddressableEntity {
                state_root_hash,
                entity_addr,
                responder,
            } => {
                trace!(?state_root_hash, "get addressable entity");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let entity_key = match entity_addr {
                        EntityAddr::SmartContract(_) | EntityAddr::System(_) => Key::AddressableEntity(entity_addr),
                        EntityAddr::Account(account) => Key::Account(AccountHash::new(account)),
                    };
                    let request = AddressableEntityRequest::new(state_root_hash, entity_key);
                    let result = data_access_layer.addressable_entity(request);
                    let result = match &result {
                        AddressableEntityResult::ValueNotFound(msg) => {
                            if entity_addr.is_contract() {
                                trace!(%msg, "can not read addressable entity by Key::AddressableEntity or Key::Account, will try by Key::Hash");
                                let entity_key = Key::Hash(entity_addr.value());
                                let request = AddressableEntityRequest::new(state_root_hash, entity_key);
                                data_access_layer.addressable_entity(request)
                            }
                            else {
                                result
                            }
                        },
                        AddressableEntityResult::RootNotFound |
                        AddressableEntityResult::Success { .. } |
                        AddressableEntityResult::Failure(_) => result,
                    };

                    metrics
                        .addressable_entity
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get addressable entity");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetEntryPoint {
                state_root_hash,
                key,
                responder,
            } => {
                trace!(?state_root_hash, "get entry point");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let request = EntryPointsRequest::new(state_root_hash, key);
                    let result = data_access_layer.entry_point(request);
                    metrics.entry_points.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get addressable entity");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetTaggedValues {
                request: tagged_values_request,
                responder,
            } => {
                trace!(?tagged_values_request, "tagged values request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.tagged_values(tagged_values_request);
                    metrics
                        .get_all_values
                        .observe(start.elapsed().as_secs_f64());
                    trace!(?result, "get all values result");
                    responder.respond(result).await
                }
                .ignore()
            }
            // trie related events
            ContractRuntimeRequest::GetTrie {
                request: trie_request,
                responder,
            } => {
                trace!(?trie_request, "trie request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.trie(trie_request);
                    metrics.get_trie.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::PutTrie {
                request: put_trie_request,
                responder,
            } => {
                trace!(?put_trie_request, "put trie request");
                let metrics = Arc::clone(&self.metrics);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                async move {
                    let start = Instant::now();
                    let result = data_access_layer.put_trie(put_trie_request);
                    let flush_req = FlushRequest::new();
                    // PERF: consider flushing periodically.
                    if let FlushResult::Failure(gse) = data_access_layer.flush(flush_req) {
                        fatal!(effect_builder, "error flushing data environment {:?}", gse).await;
                    }
                    metrics.put_trie.observe(start.elapsed().as_secs_f64());
                    trace!(?result, "put trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::UpdatePreState { new_pre_state } => {
                let next_block_height = new_pre_state.next_block_height();
                self.set_initial_state(new_pre_state);
                async move {
                    let block_header = match effect_builder
                        .get_highest_complete_block_header_from_storage()
                        .await
                    {
                        Some(header)
                            if header.is_switch_block()
                                && (header.height() + 1 == next_block_height) =>
                        {
                            header
                        }
                        Some(_) => {
                            return fatal!(
                                effect_builder,
                                "Latest complete block is not a switch block to update state"
                            )
                            .await;
                        }
                        None => {
                            return fatal!(
                                effect_builder,
                                "No complete block header found to update post upgrade state"
                            )
                            .await;
                        }
                    };

                    let finalized_block = FinalizedBlock::new(
                        BlockPayload::default(),
                        Some(InternalEraReport::default()),
                        block_header.timestamp(),
                        block_header.next_block_era_id(),
                        next_block_height,
                        PublicKey::System,
                    );

                    info!("Enqueuing block for execution post state refresh");
                    let next_era_gas_price = block_header.next_era_gas_price();
                    debug!(next_era_gas_price, "immediate switch block");

                    effect_builder
                        .enqueue_block_for_execution(
                            ExecutableBlock::from_finalized_block_and_transactions(
                                finalized_block,
                                vec![],
                                next_era_gas_price,
                            ),
                            MetaBlockState::new_not_to_be_gossiped(),
                        )
                        .await;
                }
                .ignore()
            }
            ContractRuntimeRequest::DoProtocolUpgrade {
                protocol_upgrade_config,
                next_block_height,
                parent_hash,
                parent_seed,
            } => {
                let mut effects = Effects::new();
                let data_access_layer = Arc::clone(&self.data_access_layer);
                let metrics = Arc::clone(&self.metrics);
                effects.extend(
                    handle_protocol_upgrade(
                        effect_builder,
                        data_access_layer,
                        metrics,
                        protocol_upgrade_config,
                        next_block_height,
                        parent_hash,
                        parent_seed,
                    )
                    .ignore(),
                );
                effects
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
                let next_block_height = current_pre_state.next_block_height();
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
                    // This is a future block, we store it into exec_queue, to be executed later:
                    Ordering::Greater => {
                        debug!(
                            "ContractRuntime: enqueuing({}) waiting for({})",
                            finalized_block_height, next_block_height
                        );
                        info!(
                            "ContractRuntime: enqueuing finalized block({}) with {} transactions \
                            for execution",
                            finalized_block_height,
                            executable_block.transactions.len()
                        );
                        exec_queue.insert(
                            finalized_block_height,
                            QueueItem {
                                executable_block,
                                meta_block_state,
                            },
                        );
                    }
                    // This is the next block to be executed, we do it right away:
                    Ordering::Equal => {
                        info!(
                            "ContractRuntime: execute finalized block({}) with {} transactions",
                            finalized_block_height,
                            executable_block.transactions.len()
                        );
                        let data_access_layer = Arc::clone(&self.data_access_layer);
                        let execution_engine_v1 = Arc::clone(&self.execution_engine_v1);
                        let chainspec = Arc::clone(&self.chainspec);
                        let metrics = Arc::clone(&self.metrics);
                        let shared_pre_state = Arc::clone(&self.execution_pre_state);
                        let current_gas_price = self.current_gas_price.gas_price();
                        effects.extend(
                            exec_or_requeue(
                                data_access_layer,
                                execution_engine_v1,
                                chainspec,
                                metrics,
                                exec_queue,
                                shared_pre_state,
                                current_pre_state.clone(),
                                effect_builder,
                                executable_block,
                                key_block_height_for_activation_point,
                                meta_block_state,
                                current_gas_price,
                            )
                            .ignore(),
                        )
                    }
                }
                self.metrics
                    .exec_queue_size
                    .set(self.exec_queue.len().try_into().unwrap_or(i64::MIN));
                effects
            }
            ContractRuntimeRequest::SpeculativelyExecute {
                block_header,
                transaction,
                responder,
            } => {
                let chainspec = Arc::clone(&self.chainspec);
                let data_access_layer = Arc::clone(&self.data_access_layer);
                let execution_engine_v1 = Arc::clone(&self.execution_engine_v1);
                async move {
                    let result = run_intensive_task(move || {
                        speculatively_execute(
                            data_access_layer.as_ref(),
                            chainspec.as_ref(),
                            execution_engine_v1.as_ref(),
                            *block_header,
                            *transaction,
                        )
                    })
                    .await;
                    responder.respond(result).await
                }
                .ignore()
            }
            ContractRuntimeRequest::GetEraGasPrice { era_id, responder } => responder
                .respond(self.current_gas_price.maybe_gas_price_for_era_id(era_id))
                .ignore(),
            ContractRuntimeRequest::UpdateRuntimePrice(era_id, new_gas_price) => {
                self.current_gas_price = EraPrice::new(era_id, new_gas_price);
                Effects::new()
            }
        }
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
        let TrieRequestMessage(ref serialized_id) = *message;
        let fetch_response = match self.fetch_trie_local(serialized_id) {
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
        let TrieRequestMessage(ref serialized_id) = *request_msg;
        let fetch_response = match self.fetch_trie_local(serialized_id) {
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

    /// Reads the trie (or chunk of a trie) under the given key and index.
    fn fetch_trie_local(
        &self,
        serialized_id: &[u8],
    ) -> Result<FetchResponse<TrieOrChunk, TrieOrChunkId>, ContractRuntimeError> {
        trace!(?serialized_id, "get_trie");
        let trie_or_chunk_id: TrieOrChunkId = bincode::deserialize(serialized_id)?;
        let data_access_layer = Arc::clone(&self.data_access_layer);
        let maybe_trie = {
            let start = Instant::now();
            let TrieOrChunkId(chunk_index, trie_key) = trie_or_chunk_id;
            let req = TrieRequest::new(trie_key, Some(chunk_index));
            let maybe_raw = data_access_layer
                .trie(req)
                .into_legacy()
                .map_err(ContractRuntimeError::FailedToRetrieveTrieById)?;
            let ret = match maybe_raw {
                Some(raw) => Some(TrieOrChunk::new(raw.into(), chunk_index)?),
                None => None,
            };
            self.metrics.get_trie.observe(start.elapsed().as_secs_f64());
            ret
        };
        Ok(FetchResponse::from_opt(trie_or_chunk_id, maybe_trie))
    }

    /// Returns data_access_layer, for testing only.
    #[cfg(test)]
    pub(crate) fn data_access_layer(&self) -> Arc<DataAccessLayer<LmdbGlobalState>> {
        Arc::clone(&self.data_access_layer)
    }

    #[cfg(test)]
    pub(crate) fn current_era_price(&self) -> EraPrice {
        self.current_gas_price
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

    fn name(&self) -> &str {
        COMPONENT_NAME
    }

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
