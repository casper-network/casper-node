//! Main reactor for nodes.

mod config;
mod control;
mod error;
mod event;
mod fetchers;
mod memory_metrics;
mod utils;

mod catch_up;
mod genesis_instruction;
mod keep_up;
mod reactor_state;
#[cfg(test)]
mod tests;
mod upgrade_shutdown;
mod upgrading_instruction;
mod validate;

use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use datasize::DataSize;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use tracing::{debug, error, info, warn};

use casper_types::{
    Block, BlockHash, BlockV2, Chainspec, ChainspecRawBytes, DeployId, EraId, FinalitySignature,
    PublicKey, TimeDiff, Timestamp, Transaction, TransactionHash, TransactionHeader, TransactionId,
    U512,
};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        block_accumulator::{self, BlockAccumulator},
        block_synchronizer::{self, BlockSynchronizer},
        block_validator::{self, BlockValidator},
        consensus::{self, EraSupervisor},
        contract_runtime::ContractRuntime,
        deploy_buffer::{self, DeployBuffer},
        diagnostics_port::DiagnosticsPort,
        event_stream_server::{self, EventStreamServer},
        gossiper::{self, GossipItem, Gossiper},
        metrics::Metrics,
        network::{self, GossipedAddress, Identity as NetworkIdentity, Network},
        rest_server::RestServer,
        rpc_server::RpcServer,
        shutdown_trigger::{self, CompletedBlockInfo, ShutdownTrigger},
        storage::Storage,
        sync_leaper::SyncLeaper,
        transaction_acceptor::{self, TransactionAcceptor},
        upgrade_watcher::{self, UpgradeWatcher},
        Component, ValidatorBoundComponent,
    },
    effect::{
        announcements::{
            BlockAccumulatorAnnouncement, ConsensusAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployBufferAnnouncement, FetchedNewBlockAnnouncement,
            FetchedNewFinalitySignatureAnnouncement, GossiperAnnouncement, MetaBlockAnnouncement,
            PeerBehaviorAnnouncement, TransactionAcceptorAnnouncement, UnexecutedBlockAnnouncement,
            UpgradeWatcherAnnouncement,
        },
        incoming::{NetResponseIncoming, TrieResponseIncoming},
        requests::{AcceptTransactionRequest, ChainspecRawBytesRequest},
        EffectBuilder, EffectExt, Effects, GossipTarget,
    },
    failpoints::FailpointActivation,
    fatal,
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        main_reactor::{fetchers::Fetchers, upgrade_shutdown::SignatureGossipTracker},
        EventQueueHandle, QueueKind,
    },
    types::{
        ForwardMetaBlock, MetaBlock, MetaBlockState, SyncHandling, TrieOrChunk, ValidatorMatrix,
    },
    utils::{Source, WithDir},
    NodeRng,
};
pub use config::Config;
pub(crate) use error::Error;
pub(crate) use event::MainEvent;
pub(crate) use reactor_state::ReactorState;

/// Main node reactor.
///
/// This following diagram represents how the components involved in the **sync process** interact
/// with each other.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     G((Network))
///     E((BlockAccumulator))
///     H[(Storage)]
///     I((SyncLeaper))
///     A(("Reactor<br/>(control logic)"))
///     B((ContractRuntime))
///     C((BlockSynchronizer))
///     D((Consensus))
///     K((Gossiper))
///     J((Fetcher))
///     F((DeployBuffer))
///
///     I -->|"❌<br/>Never get<br/>SyncLeap<br/>from storage"| H
///     linkStyle 0 fill:none,stroke:red,color:red
///
///     A -->|"Execute block<br/>(genesis or upgrade)"| B
///
///     G -->|Peers| C
///     G -->|Peers| D
///
///     C -->|Block data| E
///
///     J -->|Block data| C
///
///     D -->|Execute block| B
///
///     A -->|SyncLeap| I
///
///     B -->|Put block| H
///     C -->|Mark block complete| H
///     E -->|Mark block complete| H
///     C -->|Execute block| B
///
///     C -->|Complete block<br/>with Deploys| F
///
///     K -->|Deploy| F
///     K -->|Block data| E
/// ```
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
    shutdown_trigger: ShutdownTrigger,
    net: Network<MainEvent, Message>,
    consensus: EraSupervisor,

    // block handling
    block_validator: BlockValidator,
    block_accumulator: BlockAccumulator,
    block_synchronizer: BlockSynchronizer,

    // transaction handling
    transaction_acceptor: TransactionAcceptor,
    deploy_buffer: DeployBuffer,

    // gossiping components
    address_gossiper: Gossiper<{ GossipedAddress::ID_IS_COMPLETE_ITEM }, GossipedAddress>,
    transaction_gossiper: Gossiper<{ Transaction::ID_IS_COMPLETE_ITEM }, Transaction>,
    block_gossiper: Gossiper<{ BlockV2::ID_IS_COMPLETE_ITEM }, BlockV2>,
    finality_signature_gossiper:
        Gossiper<{ FinalitySignature::ID_IS_COMPLETE_ITEM }, FinalitySignature>,

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

    last_progress: Timestamp,
    attempts: usize,
    idle_tolerance: TimeDiff,
    control_logic_default_delay: TimeDiff,
    shutdown_for_upgrade_timeout: TimeDiff,
    switched_to_shutdown_for_upgrade: Timestamp,
    upgrade_timeout: TimeDiff,
    sync_handling: SyncHandling,
    signature_gossip_tracker: SignatureGossipTracker,

    finality_signature_creation: bool,
}

impl reactor::Reactor for MainReactor {
    type Event = MainEvent;
    type Config = WithDir<Config>;
    type Error = Error;

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
            MainEvent::SetNodeStopRequest(req) => reactor::wrap_effects(
                MainEvent::ShutdownTrigger,
                self.shutdown_trigger
                    .handle_event(effect_builder, rng, req.into()),
            ),

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

            MainEvent::MainReactorRequest(req) => {
                req.0.respond((self.state, self.last_progress)).ignore()
            }
            MainEvent::MetaBlockAnnouncement(MetaBlockAnnouncement(meta_block)) => self
                .handle_meta_block(
                    effect_builder,
                    rng,
                    self.finality_signature_creation,
                    meta_block,
                ),
            MainEvent::UnexecutedBlockAnnouncement(UnexecutedBlockAnnouncement(block_height)) => {
                let only_from_available_block_range = true;
                if let Ok(Some(block_header)) = self
                    .storage
                    .read_block_header_by_height(block_height, only_from_available_block_range)
                {
                    let block_hash = block_header.block_hash();
                    reactor::wrap_effects(
                        MainEvent::Consensus,
                        self.consensus.handle_event(
                            effect_builder,
                            rng,
                            consensus::Event::BlockAdded {
                                header: Box::new(block_header),
                                header_hash: block_hash,
                            },
                        ),
                    )
                } else {
                    // Warn logging here because this codepath of handling an
                    // `UnexecutedBlockAnnouncement` is coming from the
                    // contract runtime when a block with a lower height than
                    // the next expected executable height is enqueued. This
                    // happens after restarts when consensus is creating the
                    // required eras and attempts to retrace its steps in the
                    // era by enqueuing all finalized blocks starting from the
                    // first one in that era, blocks which should have already
                    // been executed and marked complete in storage.
                    warn!(
                        block_height,
                        "Finalized block enqueued for execution, but a complete \
                        block header with the same height is not present in storage."
                    );
                    Effects::new()
                }
            }

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
                // register activation point of upgrade w/ block accumulator
                self.block_accumulator
                    .register_activation_point(next_upgrade.activation_point());
                reactor::wrap_effects(
                    MainEvent::UpgradeWatcher,
                    self.upgrade_watcher.handle_event(
                        effect_builder,
                        rng,
                        upgrade_watcher::Event::GotNextUpgrade(next_upgrade),
                    ),
                )
            }
            MainEvent::RpcServer(event) => reactor::wrap_effects(
                MainEvent::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
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
            MainEvent::ShutdownTrigger(event) => reactor::wrap_effects(
                MainEvent::ShutdownTrigger,
                self.shutdown_trigger
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
            ),

            // NETWORK CONNECTION AND ORIENTATION
            MainEvent::Network(event) => reactor::wrap_effects(
                MainEvent::Network,
                self.net.handle_event(effect_builder, rng, event),
            ),
            MainEvent::NetworkRequest(req) => {
                let event = MainEvent::Network(network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::NetworkInfoRequest(req) => {
                let event = MainEvent::Network(network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::NetworkPeerBehaviorAnnouncement(ann) => {
                let mut effects = Effects::new();
                match &ann {
                    PeerBehaviorAnnouncement::OffenseCommitted {
                        offender,
                        justification: _,
                    } => {
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
            MainEvent::AddressGossiperAnnouncement(gossiper_ann) => match gossiper_ann {
                GossiperAnnouncement::GossipReceived { .. }
                | GossiperAnnouncement::NewItemBody { .. }
                | GossiperAnnouncement::FinishedGossiping(_) => Effects::new(),
                GossiperAnnouncement::NewCompleteItem(gossiped_address) => {
                    let reactor_event =
                        MainEvent::Network(network::Event::PeerAddressReceived(gossiped_address));
                    self.dispatch_event(effect_builder, rng, reactor_event)
                }
            },
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
            MainEvent::ConsensusDemand(demand) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus
                    .handle_event(effect_builder, rng, demand.into()),
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
                                public_key,
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
                BlockAccumulatorAnnouncement::AcceptedNewFinalitySignature { finality_signature },
            ) => {
                debug!(
                    "notifying finality signature gossiper to start gossiping for: {} , {}",
                    finality_signature.block_hash(),
                    finality_signature.public_key(),
                );
                let mut effects = reactor::wrap_effects(
                    MainEvent::FinalitySignatureGossiper,
                    self.finality_signature_gossiper.handle_event(
                        effect_builder,
                        rng,
                        gossiper::Event::ItemReceived {
                            item_id: finality_signature.gossip_id(),
                            source: Source::Ourself,
                            target: finality_signature.gossip_target(),
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
                        block: Arc::new(*item),
                        sender,
                    },
                ),
            ),
            MainEvent::BlockGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_block_id,
            )) => Effects::new(),
            MainEvent::BlockFetcherAnnouncement(FetchedNewBlockAnnouncement { block, peer }) => {
                // The block accumulator shouldn't concern itself with historical blocks that are
                // being fetched. If the block is not convertible to the current version it means
                // that it is surely a historical block.
                if let Ok(block) = (*block).clone().try_into() {
                    reactor::wrap_effects(
                        MainEvent::BlockAccumulator,
                        self.block_accumulator.handle_event(
                            effect_builder,
                            rng,
                            block_accumulator::Event::ReceivedBlock {
                                block: Arc::new(block),
                                sender: peer,
                            },
                        ),
                    )
                } else {
                    Effects::new()
                }
            }

            MainEvent::FinalitySignatureIncoming(incoming) => {
                // Finality signature received via broadcast.
                let sender = incoming.sender;
                let finality_signature = incoming.message;
                debug!(
                    "FinalitySignatureIncoming({},{},{},{})",
                    finality_signature.era_id(),
                    finality_signature.block_hash(),
                    finality_signature.public_key(),
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
                        block_hash: *gossiped_finality_signature_id.block_hash(),
                        era_id: Some(gossiped_finality_signature_id.era_id()),
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
                GossiperAnnouncement::FinishedGossiping(gossiped_finality_signature_id),
            ) => {
                self.signature_gossip_tracker
                    .register_signature(gossiped_finality_signature_id);
                Effects::new()
            }
            MainEvent::FinalitySignatureFetcherAnnouncement(
                FetchedNewFinalitySignatureAnnouncement {
                    finality_signature,
                    peer,
                },
            ) => reactor::wrap_effects(
                MainEvent::BlockAccumulator,
                self.block_accumulator.handle_event(
                    effect_builder,
                    rng,
                    block_accumulator::Event::ReceivedFinalitySignature {
                        finality_signature,
                        sender: peer,
                    },
                ),
            ),

            // DEPLOYS
            MainEvent::TransactionAcceptor(event) => reactor::wrap_effects(
                MainEvent::TransactionAcceptor,
                self.transaction_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::AcceptTransactionRequest(AcceptTransactionRequest {
                transaction,
                speculative_exec_at_block,
                responder,
            }) => {
                let source = if let Some(block) = speculative_exec_at_block {
                    Source::SpeculativeExec(block)
                } else {
                    Source::Client
                };
                let event = transaction_acceptor::Event::Accept {
                    transaction,
                    source,
                    maybe_responder: Some(responder),
                };
                reactor::wrap_effects(
                    MainEvent::TransactionAcceptor,
                    self.transaction_acceptor
                        .handle_event(effect_builder, rng, event),
                )
            }
            MainEvent::TransactionAcceptorAnnouncement(
                TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                    transaction,
                    source,
                },
            ) => {
                let mut effects = Effects::new();

                match source {
                    Source::Ourself => (), // internal activity does not require further action
                    Source::Peer(_) => {
                        // this is a response to a deploy fetch request, dispatch to fetcher
                        effects.extend(self.fetchers.dispatch_fetcher_event(
                            effect_builder,
                            rng,
                            MainEvent::TransactionAcceptorAnnouncement(
                                TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                                    transaction,
                                    source,
                                },
                            ),
                        ));
                    }
                    Source::Client | Source::PeerGossiped(_) => {
                        // we must attempt to gossip onwards
                        effects.extend(self.dispatch_event(
                            effect_builder,
                            rng,
                            MainEvent::TransactionGossiper(gossiper::Event::ItemReceived {
                                item_id: transaction.gossip_id(),
                                source,
                                target: transaction.gossip_target(),
                            }),
                        ));
                        // notify event stream
                        effects.extend(self.dispatch_event(
                            effect_builder,
                            rng,
                            MainEvent::EventStreamServer(
                                event_stream_server::Event::TransactionAccepted(Arc::clone(
                                    &transaction,
                                )),
                            ),
                        ));
                    }
                    Source::SpeculativeExec(_) => {
                        error!(
                            %transaction,
                            "transaction acceptor should not announce speculative exec transactions"
                        );
                    }
                }

                effects
            }
            MainEvent::TransactionAcceptorAnnouncement(
                TransactionAcceptorAnnouncement::InvalidTransaction {
                    transaction: _,
                    source: _,
                },
            ) => Effects::new(),
            MainEvent::TransactionGossiper(event) => reactor::wrap_effects(
                MainEvent::TransactionGossiper,
                self.transaction_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::TransactionGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::TransactionGossiper,
                self.transaction_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::TransactionGossiperAnnouncement(GossiperAnnouncement::GossipReceived {
                ..
            }) => {
                // Ignore the announcement.
                Effects::new()
            }
            MainEvent::TransactionGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_deploy_id,
            )) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new transaction");
                Effects::new()
            }
            MainEvent::TransactionGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                item,
                sender,
            }) => reactor::wrap_effects(
                MainEvent::TransactionAcceptor,
                self.transaction_acceptor.handle_event(
                    effect_builder,
                    rng,
                    transaction_acceptor::Event::Accept {
                        transaction: *item,
                        source: Source::PeerGossiped(sender),
                        maybe_responder: None,
                    },
                ),
            ),
            MainEvent::TransactionGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(gossiped_txn_id),
            ) => match gossiped_txn_id {
                TransactionId::Deploy {
                    deploy_hash,
                    approvals_hash,
                } => {
                    let deploy_id = DeployId::new(deploy_hash, approvals_hash);
                    let reactor_event = MainEvent::DeployBuffer(
                        deploy_buffer::Event::ReceiveDeployGossiped(deploy_id),
                    );
                    self.dispatch_event(effect_builder, rng, reactor_event)
                }
                TransactionId::V1 { .. } => {
                    todo!("avoid match `gossiped_txn_id` once deploy buffer handles transactions");
                }
            },
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
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::TransactionsExpired(
                        hashes.into_iter().map(TransactionHash::Deploy).collect(),
                    ));
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
                ContractRuntimeAnnouncement::CommitStepSuccess { era_id, effects },
            ) => {
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::Step {
                        era_id,
                        execution_effects: effects,
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
            MainEvent::MarkBlockCompletedRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::MakeBlockExecutableRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),

            // This event gets emitted when we manage to read the era validators from the global
            // states of a block after an upgrade and its parent. Once that happens, we can check
            // for the signs of any changes happening during the upgrade and register the correct
            // set of validators in the validators matrix.
            MainEvent::GotBlockAfterUpgradeEraValidators(
                era_id,
                parent_era_validators,
                block_era_validators,
            ) => {
                // `era_id`, being the era of the block after the upgrade, will be absent in the
                // validators stored in the block after the upgrade - therefore we will use its
                // successor for the comparison.
                let era_to_check = era_id.successor();
                // We read the validators for era_id+1 from the parent of the block after the
                // upgrade.
                let validators_in_parent = match parent_era_validators.get(&era_to_check) {
                    Some(validators) => validators,
                    None => {
                        return fatal!(
                            effect_builder,
                            "couldn't find validators for era {} in parent_era_validators",
                            era_to_check
                        )
                        .ignore();
                    }
                };
                // We also read the validators from the block after the upgrade itself.
                let validators_in_block = match block_era_validators.get(&era_to_check) {
                    Some(validators) => validators,
                    None => {
                        return fatal!(
                            effect_builder,
                            "couldn't find validators for era {} in block_era_validators",
                            era_to_check
                        )
                        .ignore();
                    }
                };
                // Decide which validators to use for `era_id` in the validators matrix.
                let validators_to_register = if validators_in_parent == validators_in_block {
                    // Nothing interesting happened - register the regular validators, ie. the
                    // ones stored for `era_id` in the parent of the block after the upgrade.
                    match parent_era_validators.get(&era_id) {
                        Some(validators) => validators,
                        None => {
                            return fatal!(
                                effect_builder,
                                "couldn't find validators for era {} in parent_era_validators",
                                era_id
                            )
                            .ignore();
                        }
                    }
                } else {
                    // We had an upgrade changing the validators! We use the same validators that
                    // will be used for the era after the upgrade, as we can't trust the ones we
                    // would use normally.
                    validators_in_block
                };
                let mut effects = self.update_validator_weights(
                    effect_builder,
                    rng,
                    era_id,
                    validators_to_register.clone(),
                );
                // Crank the reactor so that any synchronizing tasks blocked by the lack of
                // validators for `era_id` can resume.
                effects.extend(
                    effect_builder
                        .immediately()
                        .event(|_| MainEvent::ReactorCrank),
                );
                effects
            }

            // DELEGATE ALL FETCHER RELEVANT EVENTS to self.fetchers.dispatch_fetcher_event(..)
            MainEvent::LegacyDeployFetcher(..)
            | MainEvent::LegacyDeployFetcherRequest(..)
            | MainEvent::BlockFetcher(..)
            | MainEvent::BlockFetcherRequest(..)
            | MainEvent::TransactionFetcher(..)
            | MainEvent::TransactionFetcherRequest(..)
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
            chainspec.core_config.finality_threshold_fraction,
            chainspec
                .protocol_config
                .global_state_update
                .as_ref()
                .and_then(|global_state_update| global_state_update.validators.clone()),
            chainspec.protocol_config.activation_point.era_id(),
            our_secret_key.clone(),
            our_public_key.clone(),
            chainspec.core_config.auction_delay,
        );

        let storage_config = WithDir::new(&root_dir, config.storage.clone());

        let hard_reset_to_start_of_era = chainspec.hard_reset_to_start_of_era();
        let storage = Storage::new(
            &storage_config,
            hard_reset_to_start_of_era,
            protocol_version,
            chainspec.protocol_config.activation_point.era_id(),
            &chainspec.network_config.name,
            chainspec.transaction_config.max_ttl.into(),
            chainspec.core_config.recent_era_count(),
            Some(registry),
            config.node.force_resync,
        )?;

        let contract_runtime = ContractRuntime::new(
            storage.root_path(),
            &config.contract_runtime,
            chainspec.clone(),
            registry,
        )?;

        let network = Network::new(
            config.network.clone(),
            network_identity,
            Some((our_secret_key, our_public_key)),
            registry,
            chainspec.as_ref(),
            validator_matrix.clone(),
        )?;

        let address_gossiper = Gossiper::<{ GossipedAddress::ID_IS_COMPLETE_ITEM }, _>::new(
            "address_gossiper",
            config.gossip,
            registry,
        )?;

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
        let shutdown_trigger = ShutdownTrigger::new();

        // local / remote data management
        let sync_leaper = SyncLeaper::new(chainspec.clone(), registry)?;
        let fetchers = Fetchers::new(&config.fetcher, registry)?;

        // gossipers
        let block_gossiper = Gossiper::<{ BlockV2::ID_IS_COMPLETE_ITEM }, _>::new(
            "block_gossiper",
            config.gossip,
            registry,
        )?;
        let transaction_gossiper = Gossiper::<{ Transaction::ID_IS_COMPLETE_ITEM }, _>::new(
            "transaction_gossiper",
            config.gossip,
            registry,
        )?;
        let finality_signature_gossiper =
            Gossiper::<{ FinalitySignature::ID_IS_COMPLETE_ITEM }, _>::new(
                "finality_signature_gossiper",
                config.gossip,
                registry,
            )?;

        // consensus
        let consensus = EraSupervisor::new(
            storage.root_path(),
            validator_matrix.clone(),
            config.consensus,
            chainspec.clone(),
            registry,
        )?;

        // chain / deploy management

        let block_accumulator = BlockAccumulator::new(
            config.block_accumulator,
            validator_matrix.clone(),
            chainspec.core_config.unbonding_delay,
            chainspec.core_config.minimum_block_time,
            chainspec.core_config.validator_slots,
            registry,
        )?;
        let block_synchronizer = BlockSynchronizer::new(
            config.block_synchronizer,
            chainspec.clone(),
            chainspec.core_config.simultaneous_peer_requests,
            validator_matrix.clone(),
            registry,
        )?;
        let block_validator = BlockValidator::new(Arc::clone(&chainspec), config.block_validator);
        let upgrade_watcher =
            UpgradeWatcher::new(chainspec.as_ref(), config.upgrade_watcher, &root_dir)?;
        let transaction_acceptor =
            TransactionAcceptor::new(config.transaction_acceptor, chainspec.as_ref(), registry)?;
        let deploy_buffer =
            DeployBuffer::new(chainspec.transaction_config, config.deploy_buffer, registry)?;

        let reactor = MainReactor {
            chainspec,
            chainspec_raw_bytes,
            storage,
            contract_runtime,
            upgrade_watcher,
            net: network,
            address_gossiper,

            rpc_server,
            rest_server,
            event_stream_server,
            transaction_acceptor,
            fetchers,

            block_gossiper,
            transaction_gossiper,
            finality_signature_gossiper,
            sync_leaper,
            deploy_buffer,
            consensus,
            block_validator,
            block_accumulator,
            block_synchronizer,
            diagnostics_port,
            shutdown_trigger,

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
            sync_handling: config.node.sync_handling,
            signature_gossip_tracker: SignatureGossipTracker::new(),
            shutdown_for_upgrade_timeout: config.node.shutdown_for_upgrade_timeout,
            switched_to_shutdown_for_upgrade: Timestamp::from(0),
            upgrade_timeout: config.node.upgrade_timeout,
            finality_signature_creation: true,
        };
        info!("MainReactor: instantiated");
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

    fn activate_failpoint(&mut self, activation: &FailpointActivation) {
        if activation.key().starts_with("consensus") {
            <EraSupervisor as Component<MainEvent>>::activate_failpoint(
                &mut self.consensus,
                activation,
            );
        }
        if activation.key().starts_with("finality_signature_creation") {
            self.finality_signature_creation = false;
        }
    }
}

impl MainReactor {
    fn update_validator_weights(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        era_id: EraId,
        validator_weights: BTreeMap<PublicKey, U512>,
    ) -> Effects<MainEvent> {
        self.validator_matrix
            .register_validator_weights(era_id, validator_weights);
        info!(%era_id, "validator_matrix updated");
        // notify validator bound components
        let mut effects = reactor::wrap_effects(
            MainEvent::BlockAccumulator,
            self.block_accumulator
                .handle_validators(effect_builder, rng),
        );
        effects.extend(reactor::wrap_effects(
            MainEvent::BlockSynchronizer,
            self.block_synchronizer
                .handle_validators(effect_builder, rng),
        ));
        effects
    }

    fn handle_meta_block(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        create_finality_signatures: bool,
        mut meta_block: MetaBlock,
    ) -> Effects<MainEvent> {
        debug!(
            "MetaBlock: handling meta block {} {} {:?}",
            meta_block.height(),
            meta_block.hash(),
            meta_block.state()
        );
        if !meta_block.state().is_stored() {
            return fatal!(
                effect_builder,
                "MetaBlock: block should be stored after execution or accumulation"
            )
            .ignore();
        }

        let mut effects = Effects::new();

        if meta_block
            .mut_state()
            .register_as_sent_to_deploy_buffer()
            .was_updated()
        {
            debug!(
                "MetaBlock: notifying deploy buffer: {} {}",
                meta_block.height(),
                meta_block.hash(),
            );

            match &meta_block {
                MetaBlock::Forward(fwd_meta_block) => {
                    effects.extend(reactor::wrap_effects(
                        MainEvent::DeployBuffer,
                        self.deploy_buffer.handle_event(
                            effect_builder,
                            rng,
                            deploy_buffer::Event::Block(Arc::clone(&fwd_meta_block.block)),
                        ),
                    ));
                }
                MetaBlock::Historical(historical_meta_block) => {
                    effects.extend(reactor::wrap_effects(
                        MainEvent::DeployBuffer,
                        self.deploy_buffer.handle_event(
                            effect_builder,
                            rng,
                            deploy_buffer::Event::VersionedBlock(Arc::clone(
                                &historical_meta_block.block,
                            )),
                        ),
                    ));
                }
            }
        }

        if let MetaBlock::Forward(forward_meta_block) = &meta_block {
            let block = forward_meta_block.block.clone();
            if meta_block
                .mut_state()
                .register_updated_validator_matrix()
                .was_updated()
            {
                if let Some(validator_weights) = block.header().next_era_validator_weights() {
                    let era_id = block.era_id();
                    let next_era_id = era_id.successor();
                    debug!(
                        "MetaBlock: updating validator matrix: {} {} {} {}",
                        block.height(),
                        block.hash(),
                        era_id,
                        next_era_id
                    );
                    effects.extend(self.update_validator_weights(
                        effect_builder,
                        rng,
                        next_era_id,
                        validator_weights.clone(),
                    ));
                }
            }

            // Validators gossip the block as soon as they deem it valid, but non-validators
            // only gossip once the block is marked complete.
            if let Some(true) = self
                .validator_matrix
                .is_self_validator_in_era(block.era_id())
            {
                debug!(
                    "MetaBlock: updating validator gossip state: {} {}",
                    block.height(),
                    block.hash(),
                );
                self.update_meta_block_gossip_state(
                    effect_builder,
                    rng,
                    block.hash(),
                    block.gossip_target(),
                    meta_block.mut_state(),
                    &mut effects,
                );
            }

            if !meta_block.state().is_executed() {
                debug!(
                    "MetaBlock: unexecuted block: {} {}",
                    block.height(),
                    block.hash(),
                );
                // We've done as much as we can on a valid but un-executed block.
                return effects;
            }

            if meta_block
                .mut_state()
                .register_we_have_tried_to_sign()
                .was_updated()
                && create_finality_signatures
            {
                // When this node is a validator in this era, sign and announce.
                if let Some(finality_signature) = self
                    .validator_matrix
                    .create_finality_signature(block.header())
                {
                    debug!(
                        %finality_signature,
                        "MetaBlock: registering finality signature: {} {}",
                        block.height(),
                        block.hash(),
                    );

                    effects.extend(reactor::wrap_effects(
                        MainEvent::Storage,
                        effect_builder
                            .put_finality_signature_to_storage(finality_signature.clone())
                            .ignore(),
                    ));

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

                    let era_id = finality_signature.era_id();
                    let payload = Message::FinalitySignature(Box::new(finality_signature));
                    effects.extend(reactor::wrap_effects(
                        MainEvent::Network,
                        effect_builder
                            .broadcast_message_to_validators(payload, era_id)
                            .ignore(),
                    ));
                }
            }
        }

        if meta_block
            .mut_state()
            .register_as_consensus_notified()
            .was_updated()
        {
            debug!(
                "MetaBlock: notifying consensus: {} {}",
                meta_block.height(),
                meta_block.hash(),
            );

            match &meta_block {
                MetaBlock::Forward(fwd_meta_block) => {
                    effects.extend(reactor::wrap_effects(
                        MainEvent::Consensus,
                        self.consensus.handle_event(
                            effect_builder,
                            rng,
                            consensus::Event::BlockAdded {
                                header: Box::new(fwd_meta_block.block.header().clone().into()),
                                header_hash: *fwd_meta_block.block.hash(),
                            },
                        ),
                    ));
                }
                MetaBlock::Historical(historical_meta_block) => {
                    // Header type is the same for now so we can use the same `BlockAdded` event;
                    // When the header will be versioned, a new event will be needed for the
                    // consensus component.
                    effects.extend(reactor::wrap_effects(
                        MainEvent::Consensus,
                        self.consensus.handle_event(
                            effect_builder,
                            rng,
                            consensus::Event::BlockAdded {
                                header: Box::new(historical_meta_block.block.clone_header()),
                                header_hash: *historical_meta_block.block.hash(),
                            },
                        ),
                    ));
                }
            }
        }

        if let MetaBlock::Forward(forward_meta_block) = &meta_block {
            let block = forward_meta_block.block.clone();
            let execution_results = forward_meta_block.execution_results.clone();

            if meta_block
                .mut_state()
                .register_as_accumulator_notified()
                .was_updated()
            {
                debug!(
                    "MetaBlock: notifying accumulator: {} {}",
                    block.height(),
                    block.hash(),
                );
                let meta_block = ForwardMetaBlock {
                    block,
                    execution_results,
                    state: *meta_block.state(),
                };
                effects.extend(reactor::wrap_effects(
                    MainEvent::BlockAccumulator,
                    self.block_accumulator.handle_event(
                        effect_builder,
                        rng,
                        block_accumulator::Event::ExecutedBlock { meta_block },
                    ),
                ));
                // We've done as much as we can for now, we need to wait for the block
                // accumulator to mark the block complete before proceeding further.
                return effects;
            }
        }

        // We *always* want to initialize the contract runtime with the highest complete block.
        // In case of an upgrade, we want the reactor to hold off in the `Upgrading` state until
        // the immediate switch block is stored and *also* marked complete.
        // This will allow the contract runtime to initialize properly (see
        // [`refresh_contract_runtime`]) when the reactor is transitioning from `CatchUp` to
        // `KeepUp`.
        if !meta_block.state().is_marked_complete() {
            error!(
                block_hash = ?meta_block.hash(),
                state = ?meta_block.state(),
                "should be a complete block after passing to accumulator"
            );
        } else {
            debug!(
                "MetaBlock: block is marked complete: {} {}",
                meta_block.height(),
                meta_block.hash(),
            );
        }

        if let MetaBlock::Forward(forward_meta_block) = &meta_block {
            let block = forward_meta_block.block.clone();

            debug!(
                "MetaBlock: update gossip state: {} {}",
                block.height(),
                block.hash(),
            );
            self.update_meta_block_gossip_state(
                effect_builder,
                rng,
                block.hash(),
                block.gossip_target(),
                meta_block.mut_state(),
                &mut effects,
            );

            if meta_block
                .mut_state()
                .register_as_synchronizer_notified()
                .was_updated()
            {
                debug!(
                    "MetaBlock: notifying block synchronizer: {} {}",
                    block.height(),
                    block.hash(),
                );
                effects.extend(reactor::wrap_effects(
                    MainEvent::BlockSynchronizer,
                    self.block_synchronizer.handle_event(
                        effect_builder,
                        rng,
                        block_synchronizer::Event::MarkBlockExecuted(*block.hash()),
                    ),
                ));
            }
        }

        debug_assert!(
            meta_block.state().verify_complete(),
            "meta block {} at height {} has invalid state: {:?}",
            meta_block.hash(),
            meta_block.height(),
            meta_block.state()
        );

        if meta_block
            .mut_state()
            .register_all_actions_done()
            .was_already_registered()
        {
            error!(
                block_hash = ?meta_block.hash(),
                state = ?meta_block.state(),
                "duplicate meta block announcement emitted"
            );
            return effects;
        }

        debug!(
            "MetaBlock: notifying event stream: {} {}",
            meta_block.height(),
            meta_block.hash(),
        );
        let versioned_block: Arc<Block> = match &meta_block {
            MetaBlock::Forward(fwd_meta_block) => Arc::new((*fwd_meta_block.block).clone().into()),
            MetaBlock::Historical(historical_meta_block) => historical_meta_block.block.clone(),
        };
        effects.extend(reactor::wrap_effects(
            MainEvent::EventStreamServer,
            self.event_stream_server.handle_event(
                effect_builder,
                rng,
                event_stream_server::Event::BlockAdded(Arc::clone(&versioned_block)),
            ),
        ));

        match &meta_block {
            MetaBlock::Forward(fwd_meta_block) => {
                for exec_artifact in fwd_meta_block.execution_results.iter() {
                    let event = event_stream_server::Event::TransactionProcessed {
                        transaction_hash: TransactionHash::Deploy(exec_artifact.deploy_hash),
                        transaction_header: Box::new(TransactionHeader::Deploy(
                            exec_artifact.deploy_header.clone(),
                        )),
                        block_hash: *fwd_meta_block.block.hash(),
                        execution_result: Box::new(exec_artifact.execution_result.clone()),
                        messages: exec_artifact.messages.clone(),
                    };

                    effects.extend(reactor::wrap_effects(
                        MainEvent::EventStreamServer,
                        self.event_stream_server
                            .handle_event(effect_builder, rng, event),
                    ));
                }
            }
            MetaBlock::Historical(historical_meta_block) => {
                for (deploy_hash, deploy_header, execution_result) in
                    historical_meta_block.execution_results.iter()
                {
                    let event = event_stream_server::Event::TransactionProcessed {
                        transaction_hash: TransactionHash::Deploy(*deploy_hash),
                        transaction_header: Box::new(TransactionHeader::Deploy(
                            deploy_header.clone(),
                        )),
                        block_hash: *historical_meta_block.block.hash(),
                        execution_result: Box::new(execution_result.clone()),
                        messages: Vec::new(),
                    };
                    effects.extend(reactor::wrap_effects(
                        MainEvent::EventStreamServer,
                        self.event_stream_server
                            .handle_event(effect_builder, rng, event),
                    ));
                }
            }
        }

        debug!(
            "MetaBlock: notifying shutdown watcher: {} {}",
            meta_block.height(),
            meta_block.hash(),
        );
        effects.extend(reactor::wrap_effects(
            MainEvent::ShutdownTrigger,
            self.shutdown_trigger.handle_event(
                effect_builder,
                rng,
                shutdown_trigger::Event::CompletedBlock(CompletedBlockInfo::new(
                    meta_block.height(),
                    meta_block.era_id(),
                    meta_block.is_switch_block(),
                )),
            ),
        ));

        effects
    }

    fn update_meta_block_gossip_state(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: &BlockHash,
        gossip_target: GossipTarget,
        state: &mut MetaBlockState,
        effects: &mut Effects<MainEvent>,
    ) {
        if state.register_as_gossiped().was_updated() {
            debug!(
                "notifying block gossiper to start gossiping for: {}",
                block_hash
            );
            effects.extend(reactor::wrap_effects(
                MainEvent::BlockGossiper,
                self.block_gossiper.handle_event(
                    effect_builder,
                    rng,
                    gossiper::Event::ItemReceived {
                        item_id: *block_hash,
                        source: Source::Ourself,
                        target: gossip_target,
                    },
                ),
            ));
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
    fn node_id(&self) -> crate::types::NodeId {
        self.net.node_id()
    }
}
