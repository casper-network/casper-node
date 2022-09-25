//! Main reactor for nodes.

mod config;
mod control;
mod error;
mod event;
mod fetchers;
mod memory_metrics;
mod utils;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use datasize::DataSize;
use itertools::Itertools;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use tracing::{error, info};

use casper_execution_engine::core::{
    engine_state,
    engine_state::{ChainspecRegistry, GenesisSuccess, UpgradeConfig, UpgradeSuccess},
};
use casper_hashing::Digest;
use casper_types::{EraId, Key, PublicKey, StoredValue, TimeDiff, Timestamp};

use crate::{
    components::{
        block_synchronizer::{self, BlockSynchronizer},
        block_validator::{self, BlockValidator},
        blocks_accumulator::{BlocksAccumulator, StartingWith, SyncInstruction},
        consensus::{self, EraReport, EraSupervisor, HighwayProtocol},
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        deploy_buffer::{self, DeployBuffer},
        diagnostics_port::{self, DiagnosticsPort},
        event_stream_server::{self, EventStreamServer},
        gossiper::{self, Gossiper},
        linear_chain::{self, LinearChainComponent},
        metrics::Metrics,
        rest_server,
        rest_server::RestServer,
        rpc_server,
        rpc_server::RpcServer,
        small_network::{self, GossipedAddress, Identity as NetworkIdentity, SmallNetwork},
        storage::{FatalStorageError, Storage},
        sync_leaper,
        sync_leaper::{LeapStatus, SyncLeaper},
        upgrade_watcher::{self, UpgradeWatcher},
        Component,
    },
    contract_runtime::ExecutionPreState,
    effect::{
        announcements::{
            ConsensusAnnouncement, ContractRuntimeAnnouncement, DeployAcceptorAnnouncement,
            DeployBufferAnnouncement, GossiperAnnouncement, LinearChainAnnouncement,
            PeerBehaviorAnnouncement, RpcServerAnnouncement, UpgradeWatcherAnnouncement,
        },
        incoming::{BlockAddedResponseIncoming, NetResponseIncoming, TrieResponseIncoming},
        requests::ChainspecRawBytesRequest,
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        main_reactor::{fetchers::Fetchers, utils::initialize_component},
        EventQueueHandle, ReactorExit,
    },
    types::{
        ActivationPoint, Block, BlockAdded, BlockHash, BlockHeader, BlockPayload, Chainspec,
        ChainspecRawBytes, Deploy, ExitCode, FinalitySignature, FinalizedBlock, Item, NodeId,
        SyncLeap, TrieOrChunk, ValidatorMatrix,
    },
    utils::{Source, WithDir},
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::MainEvent;

#[derive(DataSize, Debug)]
enum ReactorState {
    // get all components and reactor state set up on start
    Initialize,
    // orient to the network and attempt to catch up to tip
    CatchUp,
    // stay caught up with tip
    KeepUp,
    // node is currently caught up and is an active validator
    //Validate, // TODO: wire this up
}

/// Main node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct MainReactor {
    // components
    //   i/o bound components
    storage: Storage,
    contract_runtime: ContractRuntime, // TODO: handle the `set_initial_state`
    upgrade_watcher: UpgradeWatcher,
    rpc_server: RpcServer,
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    diagnostics_port: DiagnosticsPort,
    small_network: SmallNetwork<MainEvent, Message>, /* TODO: handle setting the
                                                      * `is_syncing_peer` - needs
                                                      * internal init state */
    //  non-io based components
    consensus: EraSupervisor, /* TODO: Update constructor (provide less state) and extend
                               * handler for the "block added" ann. */

    // block handling
    linear_chain: LinearChainComponent, // TODO: Maybe redundant.
    block_validator: BlockValidator,    // TODO: Maybe redundant.
    blocks_accumulator: BlocksAccumulator,
    block_synchronizer: BlockSynchronizer,

    // deploy handling
    deploy_acceptor: DeployAcceptor, /* TODO: should use
                                      * `get_highest_COMPLETE_block_header_from_storage()` */
    deploy_buffer: DeployBuffer,

    //   gossiping components
    address_gossiper: Gossiper<GossipedAddress, MainEvent>, // TODO - has its own timing belt - should it?
    deploy_gossiper: Gossiper<Deploy, MainEvent>,
    block_added_gossiper: Gossiper<BlockAdded, MainEvent>,
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
    validator_matrix: ValidatorMatrix, // TODO: fill from: sync_leap, block execution, block_added
    trusted_hash: Option<BlockHash>,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,

    //   control logic
    state: ReactorState,
    max_attempts: usize,
    attempts: usize,
    idle_tolerances: TimeDiff,
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

        let validator_matrix =
            ValidatorMatrix::new(chainspec.highway_config.finality_threshold_fraction);

        let trusted_hash = config.value().node.trusted_hash;

        let (root_dir, config) = config.into_parts();
        let storage_config = WithDir::new(&root_dir, config.storage.clone());

        let hard_reset_to_start_of_era = chainspec.hard_reset_to_start_of_era();
        let storage = Storage::new(
            &storage_config,
            chainspec.highway_config.finality_threshold_fraction,
            hard_reset_to_start_of_era,
            protocol_version,
            &chainspec.network_config.name,
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

        let upgrade_watcher = UpgradeWatcher::new(chainspec.as_ref(), &root_dir)?;

        let node_key_pair = config.consensus.load_keys(&root_dir)?;
        let small_network = SmallNetwork::new(
            config.network.clone(),
            network_identity,
            Some(node_key_pair),
            registry,
            chainspec.as_ref(),
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

        let deploy_acceptor = DeployAcceptor::new(chainspec.as_ref(), registry)?;
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, MainEvent>,
            registry,
        )?;
        let block_added_gossiper = Gossiper::new_for_partial_items(
            "block_added_gossiper",
            config.gossip,
            gossiper::get_block_added_from_storage::<BlockAdded, MainEvent>,
            registry,
        )?;
        let finality_signature_gossiper = Gossiper::new_for_partial_items(
            "finality_signature_gossiper",
            config.gossip,
            gossiper::get_finality_signature_from_storage::<FinalitySignature, MainEvent>,
            registry,
        )?;

        let sync_leaper = SyncLeaper::new(chainspec.highway_config.finality_threshold_fraction);
        let deploy_buffer = DeployBuffer::new(chainspec.deploy_config, config.deploy_buffer);

        let (our_secret_key, our_public_key) = config.consensus.load_keys(&root_dir)?;
        let next_upgrade_activation_point = upgrade_watcher.next_upgrade_activation_point();
        let consensus = EraSupervisor::new(
            storage.root_path(),
            our_secret_key,
            our_public_key,
            config.consensus,
            chainspec.clone(),
            next_upgrade_activation_point,
            registry,
            Box::new(HighwayProtocol::new_boxed),
        )?;

        let block_validator = BlockValidator::new(Arc::clone(&chainspec));
        let linear_chain = LinearChainComponent::new(
            registry,
            protocol_version,
            chainspec.core_config.auction_delay,
            chainspec.core_config.unbonding_delay,
            chainspec.highway_config.finality_threshold_fraction,
            next_upgrade_activation_point,
        )?;

        let fetchers = Fetchers::new(&config.fetcher, chainspec.as_ref(), registry)?;

        let blocks_accumulator = BlocksAccumulator::new(validator_matrix.clone());
        let block_synchronizer = BlockSynchronizer::new(
            config.block_synchronizer,
            chainspec.highway_config.finality_threshold_fraction,
        );

        let diagnostics_port =
            DiagnosticsPort::new(WithDir::new(&root_dir, config.diagnostics_port));

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
            deploy_gossiper,
            block_added_gossiper,
            finality_signature_gossiper,
            sync_leaper,
            deploy_buffer,
            consensus,
            block_validator,
            linear_chain,
            blocks_accumulator,
            block_synchronizer,
            diagnostics_port,
            metrics,
            memory_metrics,
            event_queue_metrics,
            state: ReactorState::Initialize {},
            attempts: 0,
            max_attempts: 3,
            idle_tolerances: TimeDiff::from_seconds(1200),
            trusted_hash,
            validator_matrix,
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

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.linear_chain
            .stop_for_upgrade()
            .then(|| ReactorExit::ProcessShouldExit(ExitCode::Success))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        event: MainEvent,
    ) -> Effects<MainEvent> {
        match event {
            // TODO: this may be redundant / unnecessary now
            MainEvent::ControlAnnouncement(ctrl_ann) => {
                error!("unhandled control announcement: {}", ctrl_ann);
                Effects::new()
            }

            // PRIMARY REACTOR STATE CONTROL LOGIC
            MainEvent::ReactorCrank => self.crank(effect_builder, rng),

            // ROUTE ALL INTENT TO SHUTDOWN TO THIS EVENT
            // (DON'T USE FATAL ELSEWHERE WITHIN REACTOR OR COMPONENTS)
            MainEvent::Shutdown(msg) => fatal!(
                effect_builder,
                "reactor should shut down due to error: {}",
                msg,
            )
            .ignore(),

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
            ) => {
                let reactor_event = MainEvent::UpgradeWatcher(
                    upgrade_watcher::Event::GotNextUpgrade(next_upgrade.clone()),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                let reactor_event = MainEvent::Consensus(
                    consensus::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                let reactor_event = MainEvent::LinearChain(
                    linear_chain::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
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
                    PeerBehaviorAnnouncement::OffenseCommitted(node_id) => {
                        let event = MainEvent::BlockSynchronizer(
                            block_synchronizer::Event::DisconnectFromPeer(**node_id),
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
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => {
                let reactor_event =
                    MainEvent::Network(small_network::Event::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_)) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            MainEvent::SyncLeaper(event) => reactor::wrap_effects(
                MainEvent::SyncLeaper,
                self.sync_leaper.handle_event(effect_builder, rng, event),
            ),
            MainEvent::LinearChain(event) => reactor::wrap_effects(
                MainEvent::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),
            MainEvent::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded {
                block,
                approvals_checksum,
                execution_results_checksum,
            }) => {
                let reactor_event_consensus = MainEvent::Consensus(consensus::Event::BlockAdded {
                    header: Box::new(block.header().clone()),
                    header_hash: *block.hash(),
                    approvals_checksum,
                    execution_results_checksum,
                });
                let reactor_block_gossiper_event =
                    MainEvent::BlockAddedGossiper(gossiper::Event::ItemReceived {
                        item_id: *block.hash(),
                        source: Source::Ourself,
                    });
                let reactor_event_es =
                    MainEvent::EventStreamServer(event_stream_server::Event::BlockAdded(block));
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event_es);
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    reactor_block_gossiper_event,
                ));

                effects
            }
            MainEvent::LinearChainAnnouncement(LinearChainAnnouncement::NewFinalitySignature(
                fs,
            )) => {
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::FinalitySignature(fs));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
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
                    ConsensusAnnouncement::Finalized(block) => {
                        let reactor_event =
                            MainEvent::DeployBuffer(deploy_buffer::Event::BlockFinalized(block));
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::CreatedFinalitySignature(fs) => {
                        let reactor_finality_signatures_gossiper_event =
                            MainEvent::FinalitySignatureGossiper(gossiper::Event::ItemReceived {
                                item_id: fs.id(),
                                source: Source::Ourself,
                            });
                        let mut effects = self.dispatch_event(
                            effect_builder,
                            rng,
                            MainEvent::LinearChain(linear_chain::Event::FinalitySignatureReceived(
                                fs, false,
                            )),
                        );
                        effects.extend(self.dispatch_event(
                            effect_builder,
                            rng,
                            reactor_finality_signatures_gossiper_event,
                        ));
                        effects
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
            MainEvent::BlocksAccumulator(event) => reactor::wrap_effects(
                MainEvent::BlocksAccumulator,
                self.blocks_accumulator
                    .handle_event(effect_builder, rng, event),
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
            MainEvent::BlockAddedRequestIncoming(_req) => {
                // if this gets routed to storage, we can remove this BlockAddedRequestIncoming
                // variant and just use the NetRequestIncoming
                //    OR
                // route to blocks-accumulator? to construct and send the network response
                todo!()
            }
            MainEvent::BlockAddedResponseIncoming(BlockAddedResponseIncoming {
                sender,
                message,
            }) => {
                // route to blocks-accumulator and announce once validated to route to gossiper
                // AND
                // route to fetcher
                reactor::handle_fetch_response::<Self, BlockAdded>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }
            MainEvent::BlockAddedGossiper(event) => reactor::wrap_effects(
                MainEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockAddedGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::BlockAddedGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_block_added_id,
            )) => {
                error!(%gossiped_block_added_id, "gossiper should not announce new block-added");
                Effects::new()
            }
            MainEvent::BlockAddedGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_block_added_id,
            )) => Effects::new(),

            MainEvent::FinalitySignatureGossiper(event) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::FinalitySignatureIncoming(incoming) => {
                todo!(); // route it to both the LinearChain and BlocksAccumulator
                reactor::wrap_effects(
                    MainEvent::LinearChain,
                    self.linear_chain
                        .handle_event(effect_builder, rng, incoming.into()),
                )
            }
            MainEvent::FinalitySignatureGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_finality_signature_id),
            ) => {
                error!(%gossiped_finality_signature_id, "gossiper should not announce new finality signature");
                Effects::new()
            }
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
                let deploy_footprint = match deploy.footprint() {
                    Ok(deploy_footprint) => deploy_footprint,
                    Err(error) => {
                        error!(%error, "invalid deploy");
                        return Effects::new();
                    }
                };

                let mut effects = self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployBuffer(deploy_buffer::Event::ReceiveDeploy(deploy.clone())),
                );

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployGossiper(gossiper::Event::ItemReceived {
                        item_id: *deploy.id(),
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
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_deploy_id,
            )) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            MainEvent::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_deploy_id,
            )) => {
                // TODO: notify DeployBuffer the deploy can be proposed
                // let reactor_event =
                //     MainEvent::DeployBuffer(deploy_buffer::Event::
                // BufferDeploy(gossiped_deploy_id));
                // self.dispatch_event(effect_builder, rng, reactor_event)
                Effects::new()
            }
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
                ContractRuntimeAnnouncement::LinearChainBlock {
                    block,
                    approvals_checksum,
                    execution_results_checksum,
                    execution_results,
                },
            ) => {
                let mut effects = Effects::new();
                let block_hash = *block.hash();

                // send to linear chain
                let reactor_event =
                    MainEvent::LinearChain(linear_chain::Event::NewLinearChainBlock {
                        block,
                        approvals_checksum,
                        execution_results_checksum,
                        execution_results: execution_results
                            .iter()
                            .map(|(hash, _header, results)| (*hash, results.clone()))
                            .collect(),
                    });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, deploy_header, execution_result) in execution_results {
                    let reactor_event =
                        MainEvent::EventStreamServer(event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
                        });
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
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
                // the components that need to follow the era validators should have
                // a handle on the validator matrix
                self.validator_matrix
                    .register_eras(upcoming_era_validators.clone());

                // TODO: SmallNetwork should prolly be changed to use the validator matrix as well
                self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::Network(
                        ContractRuntimeAnnouncement::UpcomingEraValidators {
                            era_that_is_ending,
                            upcoming_era_validators,
                        }
                        .into(),
                    ),
                )
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
            MainEvent::DeployFetcher(..)
            | MainEvent::DeployFetcherRequest(..)
            | MainEvent::BlockHeaderFetcher(..)
            | MainEvent::BlockHeaderFetcherRequest(..)
            | MainEvent::TrieOrChunkFetcher(..)
            | MainEvent::TrieOrChunkFetcherRequest(..)
            | MainEvent::SyncLeapFetcher(..)
            | MainEvent::SyncLeapFetcherRequest(..)
            | MainEvent::BlockAddedFetcher(..)
            | MainEvent::BlockAddedFetcherRequest(..)
            | MainEvent::FinalitySignatureFetcher(..)
            | MainEvent::FinalitySignatureFetcherRequest(..) => self
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
use crate::testing::network::NetworkedReactor;

#[cfg(test)]
impl NetworkedReactor for MainReactor {
    fn node_id(&self) -> NodeId {
        self.small_network.node_id()
    }
}
