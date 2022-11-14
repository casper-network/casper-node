//! Main reactor for nodes.

mod config;
mod control;
mod error;
mod event;
mod fetchers;
mod memory_metrics;
mod utils;

mod catch_up_instruction;
mod keep_up_instruction;
mod reactor_state;
#[cfg(test)]
mod tests;
mod upgrading_instruction;
mod validate_instruction;

use std::{cmp::Ordering, collections::HashMap, sync::Arc, time::Instant};

use datasize::DataSize;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use tracing::{debug, error, info, warn};

use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::{
        block_accumulator::{self, BlockAccumulator},
        block_synchronizer::{self, BlockSynchronizer},
        block_validator::{self, BlockValidator},
        consensus::{self, ChainspecConsensusExt, EraSupervisor, HighwayProtocol},
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        deploy_buffer::{self, DeployBuffer},
        diagnostics_port::DiagnosticsPort,
        event_stream_server::{self, EventStreamServer},
        gossiper::{self, Gossiper},
        metrics::Metrics,
        rest_server::RestServer,
        rpc_server::RpcServer,
        small_network::{self, GossipedAddress, Identity as NetworkIdentity, SmallNetwork},
        storage::Storage,
        sync_leaper::SyncLeaper,
        upgrade_watcher::{self, UpgradeWatcher},
        Component,
    },
    effect::{
        announcements::{
            BlockAccumulatorAnnouncement, ConsensusAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployAcceptorAnnouncement, DeployBufferAnnouncement,
            GossiperAnnouncement, PeerBehaviorAnnouncement, RpcServerAnnouncement,
            UpgradeWatcherAnnouncement,
        },
        incoming::{NetResponseIncoming, TrieResponseIncoming},
        requests::{BlockSynchronizerRequest, ChainspecRawBytesRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::{
        self, event_queue_metrics::EventQueueMetrics, main_reactor::fetchers::Fetchers,
        EventQueueHandle, QueueKind,
    },
    types::{
        Block, BlockHash, BlockHeader, Chainspec, ChainspecRawBytes, Deploy, FinalitySignature,
        Item, TrieOrChunk, ValidatorMatrix,
    },
    utils::{Source, WithDir},
    NodeRng,
};
#[cfg(test)]
use crate::{testing::network::NetworkedReactor, types::NodeId};
pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::MainEvent;
pub(crate) use reactor_state::ReactorState;

/// Main node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct MainReactor {
    // components
    //   i/o bound components
    storage: Storage,
    contract_runtime: ContractRuntime,
    upgrade_watcher: UpgradeWatcher,
    rpc_server: RpcServer,
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    diagnostics_port: DiagnosticsPort,
    small_network: SmallNetwork<MainEvent, Message>,
    consensus: EraSupervisor,

    // block handling
    block_validator: BlockValidator,
    block_accumulator: BlockAccumulator,
    block_synchronizer: BlockSynchronizer,

    // deploy handling
    deploy_acceptor: DeployAcceptor,
    deploy_buffer: DeployBuffer,

    // gossiping components
    address_gossiper: Gossiper<GossipedAddress, MainEvent>,
    deploy_gossiper: Gossiper<Deploy, MainEvent>,
    block_gossiper: Gossiper<Block, MainEvent>,
    finality_signature_gossiper: Gossiper<FinalitySignature, MainEvent>,

    // record retrieval
    sync_leaper: SyncLeaper,
    fetchers: Fetchers, // <-- this contains all fetchers to reduce top-level clutter

    // Non-components.
    //   metrics
    metrics: Metrics,
    #[data_size(skip)] // Never allocates heap data.
    memory_metrics: MemoryMetrics,
    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,

    //   ambient settings / data / load-bearing config
    validator_matrix: ValidatorMatrix,
    trusted_hash: Option<BlockHash>,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,

    //   control logic
    state: ReactorState,
    max_attempts: usize,
    switch_block: Option<BlockHeader>,

    last_progress: Timestamp,
    attempts: usize,
    idle_tolerance: TimeDiff,
    control_logic_default_delay: TimeDiff,
    recent_switch_block_headers: Vec<BlockHeader>,
    sync_to_historical: bool,
}

impl reactor::Reactor for MainReactor {
    type Event = MainEvent;
    type Config = WithDir<Config>;
    type Error = Error;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<MainEvent>), Error> {
        let node_startup_instant = Instant::now();

        let effect_builder = EffectBuilder::new(event_queue);

        let metrics = Metrics::new(registry.clone());
        let memory_metrics = MemoryMetrics::new(registry.clone())?;
        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let protocol_version = chainspec.protocol_config.version;

        let trusted_hash = config.value().node.trusted_hash;
        let (root_dir, config) = config.into_parts();
        let (our_secret_key, our_public_key) = config.consensus.load_keys(&root_dir)?;
        let validator_matrix = ValidatorMatrix::new(
            chainspec.highway_config.finality_threshold_fraction,
            our_secret_key.clone(),
            our_public_key.clone(),
            chainspec.core_config.auction_delay,
        );

        let storage_config = WithDir::new(&root_dir, config.storage.clone());

        let hard_reset_to_start_of_era = chainspec.hard_reset_to_start_of_era();
        let storage = Storage::new(
            &storage_config,
            chainspec.highway_config.finality_threshold_fraction,
            hard_reset_to_start_of_era,
            protocol_version,
            &chainspec.network_config.name,
            chainspec.deploy_config.max_ttl,
            chainspec.core_config.recent_era_count(),
        )?;

        let contract_runtime = ContractRuntime::new(
            protocol_version,
            storage.root_path(),
            &config.contract_runtime,
            chainspec.wasm_config,
            chainspec.system_costs_config,
            chainspec.core_config.max_associated_keys,
            chainspec.core_config.max_runtime_call_stack_height,
            chainspec.core_config.minimum_delegation_amount,
            chainspec.core_config.strict_argument_checking,
            chainspec.core_config.vesting_schedule_period.millis(),
            registry,
        )?;

        let small_network = SmallNetwork::new(
            config.network.clone(),
            network_identity,
            Some((our_secret_key.clone(), our_public_key.clone())),
            registry,
            chainspec.as_ref(),
            validator_matrix.clone(),
        )?;

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let rpc_server = RpcServer::new(
            config.rpc_server.clone(),
            config.speculative_exec_server.clone(),
            protocol_version,
            chainspec.network_config.name.clone(),
            node_startup_instant,
        );
        let rest_server = RestServer::new(
            config.rest_server.clone(),
            protocol_version,
            chainspec.network_config.name.clone(),
            node_startup_instant,
        );
        let event_stream_server = EventStreamServer::new(
            config.event_stream_server.clone(),
            storage.root_path().to_path_buf(),
            protocol_version,
        );
        let diagnostics_port =
            DiagnosticsPort::new(WithDir::new(&root_dir, config.diagnostics_port));

        // local / remote data management
        let sync_leaper = SyncLeaper::new(chainspec.clone());
        let fetchers = Fetchers::new(&config.fetcher, registry)?;

        // gossipers
        let block_gossiper = Gossiper::new_for_partial_items(
            "block_gossiper",
            config.gossip,
            gossiper::get_block_from_storage::<Block, MainEvent>,
            registry,
        )?;
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, MainEvent>,
            registry,
        )?;
        let finality_signature_gossiper = Gossiper::new_for_partial_items(
            "finality_signature_gossiper",
            config.gossip,
            gossiper::get_finality_signature_from_storage::<FinalitySignature, MainEvent>,
            registry,
        )?;

        // consensus
        let consensus = EraSupervisor::new(
            storage.root_path(),
            our_secret_key,
            our_public_key,
            config.consensus,
            chainspec.clone(),
            registry,
            Box::new(HighwayProtocol::new_boxed),
        )?;

        // chain / deploy management
        let highest_block_header = storage.read_highest_block_header()?;
        let highest_block_height_and_era_id =
            highest_block_header.map(|header| (header.height(), header.era_id()));

        let block_accumulator = BlockAccumulator::new(
            config.block_accumulator,
            validator_matrix.clone(),
            highest_block_height_and_era_id,
            chainspec.core_config.unbonding_delay,
        );
        let block_synchronizer =
            BlockSynchronizer::new(config.block_synchronizer, validator_matrix.clone());
        let block_validator = BlockValidator::new(Arc::clone(&chainspec));
        let upgrade_watcher =
            UpgradeWatcher::new(chainspec.as_ref(), config.upgrade_watcher, &root_dir)?;
        let deploy_acceptor = DeployAcceptor::new(chainspec.as_ref(), registry)?;
        let deploy_buffer = DeployBuffer::new(chainspec.deploy_config, config.deploy_buffer);
        let era_count = chainspec.number_of_past_switch_blocks_needed();
        let recent_switch_block_headers = storage.read_highest_switch_block_headers(era_count)?;

        let reactor = MainReactor {
            chainspec,
            chainspec_raw_bytes,
            storage,
            contract_runtime,
            upgrade_watcher,
            small_network,
            address_gossiper,

            rpc_server,
            rest_server,
            event_stream_server,
            deploy_acceptor,
            fetchers,

            block_gossiper,
            deploy_gossiper,
            finality_signature_gossiper,
            sync_leaper,
            deploy_buffer,
            consensus,
            block_validator,
            block_accumulator,
            block_synchronizer,
            diagnostics_port,

            metrics,
            memory_metrics,
            event_queue_metrics,

            state: ReactorState::Initialize {},
            attempts: 0,
            last_progress: Timestamp::now(),
            max_attempts: config.node.max_attempts,
            idle_tolerance: config.node.idle_tolerance,
            control_logic_default_delay: config.node.control_logic_default_delay,
            trusted_hash,
            validator_matrix,
            recent_switch_block_headers,
            switch_block: None,
            sync_to_historical: config.node.sync_to_genesis,
        };

        let effects = effect_builder
            .immediately()
            .event(|()| MainEvent::ReactorCrank);
        Ok((reactor, effects))
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle)
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        event: MainEvent,
    ) -> Effects<MainEvent> {
        match event {
            MainEvent::ControlAnnouncement(ctrl_ann) => {
                error!("unhandled control announcement: {}", ctrl_ann);
                Effects::new()
            }

            MainEvent::FatalAnnouncement(fatal_ann) => {
                if self.consensus.is_active_validator() {
                    warn!(%fatal_ann, "consensus is active, not shutting down");
                    Effects::new()
                } else {
                    let ctrl_ann =
                        MainEvent::ControlAnnouncement(ControlAnnouncement::FatalError {
                            file: fatal_ann.file,
                            line: fatal_ann.line,
                            msg: fatal_ann.msg,
                        });
                    effect_builder
                        .into_inner()
                        .schedule(ctrl_ann, QueueKind::Control)
                        .ignore()
                }
            }

            // PRIMARY REACTOR STATE CONTROL LOGIC
            MainEvent::ReactorCrank => self.crank(effect_builder, rng),

            // LOCAL I/O BOUND COMPONENTS
            MainEvent::UpgradeWatcher(event) => reactor::wrap_effects(
                MainEvent::UpgradeWatcher,
                self.upgrade_watcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::UpgradeWatcherRequest(req) => reactor::wrap_effects(
                MainEvent::UpgradeWatcher,
                self.upgrade_watcher
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::UpgradeWatcherAnnouncement(
                UpgradeWatcherAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => reactor::wrap_effects(
                MainEvent::UpgradeWatcher,
                self.upgrade_watcher.handle_event(
                    effect_builder,
                    rng,
                    upgrade_watcher::Event::GotNextUpgrade(next_upgrade),
                ),
            ),
            MainEvent::RpcServer(event) => reactor::wrap_effects(
                MainEvent::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            MainEvent::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::Client,
                    maybe_responder: responder,
                };
                self.dispatch_event(effect_builder, rng, MainEvent::DeployAcceptor(event))
            }
            MainEvent::RestServer(event) => reactor::wrap_effects(
                MainEvent::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            MainEvent::MetricsRequest(req) => reactor::wrap_effects(
                MainEvent::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            MainEvent::ChainspecRawBytesRequest(
                ChainspecRawBytesRequest::GetChainspecRawBytes(responder),
            ) => responder.respond(self.chainspec_raw_bytes.clone()).ignore(),
            MainEvent::EventStreamServer(event) => reactor::wrap_effects(
                MainEvent::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DiagnosticsPort(event) => reactor::wrap_effects(
                MainEvent::DiagnosticsPort,
                self.diagnostics_port
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DumpConsensusStateRequest(req) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, req.into()),
                // req.answer(Err(Cow::Borrowed("node is joining, no running consensus")))
                //     .ignore()
            ),

            // NETWORK CONNECTION AND ORIENTATION
            MainEvent::Network(event) => reactor::wrap_effects(
                MainEvent::Network,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            MainEvent::NetworkRequest(req) => {
                let event = MainEvent::Network(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::NetworkInfoRequest(req) => {
                let event = MainEvent::Network(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::NetworkPeerBehaviorAnnouncement(ann) => {
                let mut effects = Effects::new();
                match &ann {
                    PeerBehaviorAnnouncement::OffenseCommitted {
                        offender,
                        justification: _,
                    } => {
                        // todo!() - instead of ignoring, should we pass `justification` down to the
                        // "peer rating" system? Probably, because it'll allow us to add additional
                        // info to `BlocklistJustification::DishonestPeer` variant.
                        let event = MainEvent::BlockSynchronizer(
                            block_synchronizer::Event::DisconnectFromPeer(**offender),
                        );
                        effects.extend(self.dispatch_event(effect_builder, rng, event));
                    }
                }
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::Network(ann.into()),
                ));
                effects
            }
            MainEvent::NetworkPeerRequestingData(incoming) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::NetworkPeerProvidingData(NetResponseIncoming { sender, message }) => {
                reactor::handle_get_response(self, effect_builder, rng, sender, message)
            }
            MainEvent::AddressGossiper(event) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::AddressGossiperCrank(req) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::GossipReceived {
                item_id: gossiped_address,
                ..
            }) => {
                error!(%gossiped_address, "gossiper should not announce gossiped address");
                Effects::new()
            }
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => {
                let reactor_event =
                    MainEvent::Network(small_network::Event::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                ..
            }) => {
                // Should not be reachable.
                Effects::new()
            }
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_)) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            MainEvent::SyncLeaper(event) => reactor::wrap_effects(
                MainEvent::SyncLeaper,
                self.sync_leaper.handle_event(effect_builder, rng, event),
            ),
            MainEvent::Consensus(event) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            MainEvent::ConsensusMessageIncoming(incoming) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::ConsensusAnnouncement(consensus_announcement) => {
                match consensus_announcement {
                    ConsensusAnnouncement::Proposed(block) => {
                        let reactor_event =
                            MainEvent::DeployBuffer(deploy_buffer::Event::BlockProposed(block));
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::Finalized(block) => {
                        let reactor_event =
                            MainEvent::DeployBuffer(deploy_buffer::Event::BlockFinalized(block));
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::Fault {
                        era_id,
                        public_key,
                        timestamp,
                    } => {
                        let reactor_event =
                            MainEvent::EventStreamServer(event_stream_server::Event::Fault {
                                era_id,
                                public_key: *public_key,
                                timestamp,
                            });
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                }
            }

            // BLOCKS
            MainEvent::BlockValidator(event) => reactor::wrap_effects(
                MainEvent::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                MainEvent::BlockValidator(block_validator::Event::from(req)),
            ),
            MainEvent::BlockAccumulator(event) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockAccumulatorRequest(request) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::BlockSynchronizer(event) => reactor::wrap_effects(
                MainEvent::BlockSynchronizer,
                self.block_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockSynchronizerRequest(req) => reactor::wrap_effects(
                MainEvent::BlockSynchronizer,
                self.block_synchronizer
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::BlockAccumulatorAnnouncement(
                BlockAccumulatorAnnouncement::AcceptedNewBlock { block },
            ) => {
                let mut effects = Effects::new();

                let reactor_event_es = MainEvent::EventStreamServer(
                    event_stream_server::Event::BlockAdded(block.clone()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_es));
                let block_sync_event =
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::BlockExecuted {
                        block_hash: *block.hash(),
                        height: block.height(),
                    });
                effects.extend(self.dispatch_event(effect_builder, rng, block_sync_event));
                let deploy_buffer_event =
                    MainEvent::DeployBuffer(deploy_buffer::Event::Block(block.clone()));
                effects.extend(self.dispatch_event(effect_builder, rng, deploy_buffer_event));

                let reactor_event_consensus = MainEvent::Consensus(consensus::Event::BlockAdded {
                    header: Box::new(block.header().clone()),
                    header_hash: *block.hash(),
                });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));

                // if it is a switch block, get validators
                if let Some(validator_weights) = block.header().next_era_validator_weights() {
                    self.switch_block = Some(block.header().clone());

                    let era_id = block.header().era_id();
                    self.validator_matrix
                        .register_validator_weights(era_id.successor(), validator_weights.clone());
                    debug!(
                        "block_accumulator added switch block (notifying components of validator weights at end of: {})",
                        era_id
                    );
                    effects.extend(reactor::wrap_effects(
                        MainEvent::BlockAccumulator,
                        self.block_accumulator.handle_event(
                            effect_builder,
                            rng,
                            block_accumulator::Event::ValidatorMatrixUpdated,
                        ),
                    ));
                    effects.extend(reactor::wrap_effects(
                        MainEvent::BlockSynchronizer,
                        self.block_synchronizer.handle_event(
                            effect_builder,
                            rng,
                            block_synchronizer::Event::ValidatorMatrixUpdated,
                        ),
                    ));

                    match self.recent_switch_block_headers.last() {
                        Some(header) => {
                            match block.header().era_id().cmp(&header.era_id().successor()) {
                                Ordering::Greater => {
                                    self.recent_switch_block_headers = vec![block.header().clone()];
                                }
                                Ordering::Equal => self
                                    .recent_switch_block_headers
                                    .push(block.header().clone()),
                                Ordering::Less => {
                                    warn!("recent switch blocks has later entry than this block");
                                }
                            }
                        }
                        None => self
                            .recent_switch_block_headers
                            .push(block.header().clone()),
                    }

                    let era_count = self.chainspec.number_of_past_switch_blocks_needed();
                    while self.recent_switch_block_headers.len() as u64 > era_count {
                        self.recent_switch_block_headers.remove(0);
                    }
                }
                debug!(
                    "notifying block gossiper to start gossiping for: {}",
                    block.id()
                );
                effects.extend(reactor::wrap_effects(
                    MainEvent::BlockGossiper,
                    self.block_gossiper.handle_event(
                        effect_builder,
                        rng,
                        gossiper::Event::ItemReceived {
                            item_id: *block.hash(),
                            source: Source::Ourself,
                        },
                    ),
                ));
                effects
            }
            MainEvent::BlockAccumulatorAnnouncement(
                BlockAccumulatorAnnouncement::AcceptedNewFinalitySignature { finality_signature },
            ) => {
                debug!(
                    "notifying finality signature gossiper to start gossiping for: {} , {}",
                    finality_signature.block_hash, finality_signature.public_key,
                );
                let mut effects = reactor::wrap_effects(
                    MainEvent::FinalitySignatureGossiper,
                    self.finality_signature_gossiper.handle_event(
                        effect_builder,
                        rng,
                        gossiper::Event::ItemReceived {
                            item_id: finality_signature.id(),
                            source: Source::Ourself,
                        },
                    ),
                );

                effects.extend(reactor::wrap_effects(
                    MainEvent::EventStreamServer,
                    self.event_stream_server.handle_event(
                        effect_builder,
                        rng,
                        event_stream_server::Event::FinalitySignature(finality_signature),
                    ),
                ));

                effects
            }
            MainEvent::BlockGossiper(event) => reactor::wrap_effects(
                MainEvent::BlockGossiper,
                self.block_gossiper.handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::BlockGossiper,
                self.block_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::BlockGossiperAnnouncement(GossiperAnnouncement::GossipReceived {
                item_id: gossiped_block_id,
                sender,
            }) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator.handle_event(
                    effect_builder,
                    rng,
                    block_accumulator::Event::RegisterPeer {
                        block_hash: gossiped_block_id,
                        era_id: None,
                        sender,
                    },
                ),
            ),
            MainEvent::BlockGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_block_id,
            )) => {
                error!(%gossiped_block_id, "gossiper should not announce new block");
                Effects::new()
            }
            MainEvent::BlockGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                item,
                sender,
            }) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator.handle_event(
                    effect_builder,
                    rng,
                    block_accumulator::Event::ReceivedBlock {
                        block: item,
                        sender,
                    },
                ),
            ),
            MainEvent::BlockGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_block_id,
            )) => Effects::new(),

            MainEvent::FinalitySignatureIncoming(incoming) => {
                // Finality signature received via broadcast.
                let sender = incoming.sender;
                let finality_signature = incoming.message;
                debug!(
                    "FinalitySignatureIncoming({},{},{},{})",
                    finality_signature.era_id,
                    finality_signature.block_hash,
                    finality_signature.public_key,
                    sender
                );
                let block_accumulator_event = block_accumulator::Event::ReceivedFinalitySignature {
                    finality_signature,
                    sender,
                };
                reactor::wrap_effects(
                    MainEvent::BlockAccumulator,
                    self.block_accumulator.handle_event(
                        effect_builder,
                        rng,
                        block_accumulator_event,
                    ),
                )
            }
            MainEvent::FinalitySignatureGossiper(event) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::FinalitySignatureGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::GossipReceived {
                    item_id: gossiped_finality_signature_id,
                    sender,
                },
            ) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator.handle_event(
                    effect_builder,
                    rng,
                    block_accumulator::Event::RegisterPeer {
                        block_hash: gossiped_finality_signature_id.block_hash,
                        era_id: Some(gossiped_finality_signature_id.era_id),
                        sender,
                    },
                ),
            ),
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_finality_signature_id),
            ) => {
                error!(%gossiped_finality_signature_id, "gossiper should not announce new finality signature");
                Effects::new()
            }
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::NewItemBody { item, sender },
            ) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator.handle_event(
                    effect_builder,
                    rng,
                    block_accumulator::Event::ReceivedFinalitySignature {
                        finality_signature: item,
                        sender,
                    },
                ),
            ),
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_finality_signature_id),
            ) => Effects::new(),

            // DEPLOYS
            MainEvent::DeployAcceptor(event) => reactor::wrap_effects(
                MainEvent::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let mut effects = self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployBuffer(deploy_buffer::Event::ReceiveDeploy(deploy.clone())),
                );

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployGossiper(gossiper::Event::ItemReceived {
                        item_id: deploy.id(),
                        source: source.clone(),
                    }),
                ));

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::EventStreamServer(event_stream_server::Event::DeployAccepted(
                        deploy.clone(),
                    )),
                ));

                effects.extend(self.fetchers.dispatch_fetcher_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
                    ),
                ));

                effects
            }
            MainEvent::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            MainEvent::DeployGossiper(event) => reactor::wrap_effects(
                MainEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::GossipReceived {
                ..
            }) => {
                // Ignore the announcement.
                Effects::new()
            }
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_deploy_id,
            )) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                item,
                sender,
            }) => reactor::wrap_effects(
                MainEvent::DeployAcceptor,
                self.deploy_acceptor.handle_event(
                    effect_builder,
                    rng,
                    deploy_acceptor::Event::Accept {
                        deploy: item,
                        source: Source::Peer(sender),
                        maybe_responder: None,
                    },
                ),
            ),
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                gossiped_deploy_id,
            )) => reactor::wrap_effects(
                MainEvent::DeployBuffer,
                self.deploy_buffer.handle_event(
                    effect_builder,
                    rng,
                    deploy_buffer::Event::DeployHashGossiped(*gossiped_deploy_id.deploy_hash()),
                ),
            ),
            MainEvent::DeployBuffer(event) => reactor::wrap_effects(
                MainEvent::DeployBuffer,
                self.deploy_buffer.handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployBufferRequest(req) => {
                self.dispatch_event(effect_builder, rng, MainEvent::DeployBuffer(req.into()))
            }
            MainEvent::DeployBufferAnnouncement(DeployBufferAnnouncement::DeploysExpired(
                hashes,
            )) => {
                let reactor_event = MainEvent::EventStreamServer(
                    event_stream_server::Event::DeploysExpired(hashes),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }

            // CONTRACT RUNTIME & GLOBAL STATE
            MainEvent::ContractRuntime(event) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::ContractRuntimeRequest(req) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::ExecutedBlock {
                    block,
                    approvals_hashes,
                    execution_results,
                },
            ) => {
                let mut effects = Effects::new();
                let block_hash = *block.hash();
                let is_switch_block = block.header().is_switch_block();
                debug!(
                    %block_hash,
                    height=block.header().height(),
                    era=block.header().era_id().value(),
                    is_switch_block=is_switch_block,
                    "executed block"
                );

                if is_switch_block {
                    self.switch_block = Some(block.header().clone());
                } else {
                    self.switch_block = None;
                }

                let execution_results_map: HashMap<_, _> = execution_results
                    .iter()
                    .cloned()
                    .map(|(deploy_hash, _, execution_result)| (deploy_hash, execution_result))
                    .collect();
                effects.extend(
                    effect_builder
                        .put_executed_block_to_storage(
                            block.clone(),
                            approvals_hashes,
                            execution_results_map,
                        )
                        .ignore(),
                );

                // notify the block accumulator
                let event = block_accumulator::Event::ExecutedBlock {
                    block: block.clone(),
                };
                effects.extend(reactor::wrap_effects(
                    MainEvent::BlockAccumulator,
                    self.block_accumulator
                        .handle_event(effect_builder, rng, event),
                ));

                // When this node is a validator in this era, sign and announce.
                if let Some(finality_signature) = self
                    .validator_matrix
                    .create_finality_signature(block.header())
                {
                    // send to block accumulator
                    effects.extend(reactor::wrap_effects(
                        MainEvent::BlockAccumulator,
                        self.block_accumulator.handle_event(
                            effect_builder,
                            rng,
                            block_accumulator::Event::CreatedFinalitySignature {
                                finality_signature: Box::new(finality_signature.clone()),
                            },
                        ),
                    ));

                    // broadcast to validator peers
                    let era_id = finality_signature.era_id;
                    let payload = Message::FinalitySignature(Box::new(finality_signature));
                    effects.extend(reactor::wrap_effects(
                        MainEvent::Network,
                        effect_builder
                            .broadcast_message_to_validators(payload, era_id)
                            .ignore(),
                    ));
                }

                // send deploy processed events to event stream
                for (deploy_hash, deploy_header, execution_result) in execution_results {
                    let event = event_stream_server::Event::DeployProcessed {
                        deploy_hash,
                        deploy_header: Box::new(deploy_header),
                        block_hash,
                        execution_result: Box::new(execution_result),
                    };
                    effects.extend(reactor::wrap_effects(
                        MainEvent::EventStreamServer,
                        self.event_stream_server
                            .handle_event(effect_builder, rng, event),
                    ));
                }

                effects
            }
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::CommitStepSuccess {
                    era_id,
                    execution_effect,
                },
            ) => {
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::Step {
                        era_id,
                        execution_effect,
                    });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::UpcomingEraValidators {
                    era_that_is_ending,
                    upcoming_era_validators,
                },
            ) => {
                info!(
                    "UpcomingEraValidators era_that_is_ending: {}",
                    era_that_is_ending
                );
                self.validator_matrix.register_eras(upcoming_era_validators);
                Effects::new()
            }

            MainEvent::TrieRequestIncoming(req) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::TrieDemand(demand) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, demand.into()),
            ),
            MainEvent::TrieResponseIncoming(TrieResponseIncoming { sender, message }) => {
                reactor::handle_fetch_response::<Self, TrieOrChunk>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }

            // STORAGE
            MainEvent::Storage(event) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            MainEvent::StorageRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::AppStateRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::BlockCompleteConfirmationRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),

            // DELEGATE ALL FETCHER RELEVANT EVENTS to self.fetchers.dispatch_fetcher_event(..)
            MainEvent::LegacyDeployFetcher(..)
            | MainEvent::LegacyDeployFetcherRequest(..)
            | MainEvent::BlockFetcher(..)
            | MainEvent::BlockFetcherRequest(..)
            | MainEvent::DeployFetcher(..)
            | MainEvent::DeployFetcherRequest(..)
            | MainEvent::BlockHeaderFetcher(..)
            | MainEvent::BlockHeaderFetcherRequest(..)
            | MainEvent::TrieOrChunkFetcher(..)
            | MainEvent::TrieOrChunkFetcherRequest(..)
            | MainEvent::SyncLeapFetcher(..)
            | MainEvent::SyncLeapFetcherRequest(..)
            | MainEvent::ApprovalsHashesFetcher(..)
            | MainEvent::ApprovalsHashesFetcherRequest(..)
            | MainEvent::FinalitySignatureFetcher(..)
            | MainEvent::FinalitySignatureFetcherRequest(..)
            | MainEvent::BlockExecutionResultsOrChunkFetcher(..)
            | MainEvent::BlockExecutionResultsOrChunkFetcherRequest(..) => self
                .fetchers
                .dispatch_fetcher_event(effect_builder, rng, event),
        }
    }
}

// TEST ENABLEMENT -- used by integration tests elsewhere
#[cfg(test)]
impl MainReactor {
    pub(crate) fn consensus(&self) -> &EraSupervisor {
        &self.consensus
    }

    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    pub(crate) fn contract_runtime(&self) -> &ContractRuntime {
        &self.contract_runtime
    }
}

#[cfg(test)]
impl NetworkedReactor for MainReactor {
    fn node_id(&self) -> NodeId {
        self.small_network.node_id()
    }
}
