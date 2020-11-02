//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;
mod memory_metrics;
#[cfg(test)]
mod tests;

use std::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    str::FromStr,
};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use tracing::{debug, error, warn};

use deploy_buffer::ProtoBlockCollection;

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        api_server::{self, ApiServer},
        block_executor::{self, BlockExecutor},
        block_validator::{self, BlockValidator},
        chainspec_loader::{self, ChainspecLoader},
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor::{self, DeployAcceptor},
        deploy_buffer::{self, DeployBuffer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        metrics::Metrics,
        small_network::{self, GossipedAddress, NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            ApiServerAnnouncement, BlockExecutorAnnouncement, ConsensusAnnouncement,
            DeployAcceptorAnnouncement, GossiperAnnouncement, LinearChainAnnouncement,
            NetworkAnnouncement,
        },
        requests::{
            ApiRequest, BlockExecutorRequest, BlockValidationRequest, ChainspecLoaderRequest,
            ConsensusRequest, ContractRuntimeRequest, DeployBufferRequest, FetcherRequest,
            LinearChainRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::{self, event_queue_metrics::EventQueueMetrics, EventQueueHandle},
    types::{Block, CryptoRngCore, Deploy, ProtoBlock, Tag, TimeDiff, Timestamp},
    utils::Source,
};
pub use config::Config;
pub use error::Error;
use linear_chain::LinearChain;
use memory_metrics::MemoryMetrics;

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(small_network::Event<Message>),
    /// Deploy buffer event.
    #[from]
    DeployBuffer(deploy_buffer::Event),
    #[from]
    /// Storage event.
    Storage(storage::Event<Storage>),
    #[from]
    /// API server event.
    ApiServer(api_server::Event),
    #[from]
    /// Chainspec Loader event.
    ChainspecLoader(chainspec_loader::Event),
    #[from]
    /// Consensus event.
    Consensus(consensus::Event<NodeId>),
    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(deploy_acceptor::Event),
    /// Deploy fetcher event.
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(gossiper::Event<Deploy>),
    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),
    /// Block executor event.
    #[from]
    BlockExecutor(block_executor::Event),
    /// Block validator event.
    #[from]
    ProtoBlockValidator(block_validator::Event<ProtoBlock, NodeId>),
    /// Linear chain event.
    #[from]
    LinearChain(linear_chain::Event<NodeId>),

    // Requests
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    /// Network info request.
    #[from]
    NetworkInfoRequest(NetworkInfoRequest<NodeId>),
    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(FetcherRequest<NodeId, Deploy>),
    /// Deploy buffer request.
    #[from]
    DeployBufferRequest(DeployBufferRequest),
    /// Block executor request.
    #[from]
    BlockExecutorRequest(BlockExecutorRequest),
    /// Block validator request.
    #[from]
    ProtoBlockValidatorRequest(BlockValidationRequest<ProtoBlock, NodeId>),
    /// Metrics request.
    #[from]
    MetricsRequest(MetricsRequest),
    /// Chainspec info request
    #[from]
    ChainspecLoaderRequest(ChainspecLoaderRequest),

    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    /// API server announcement.
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement<NodeId>),
    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(ConsensusAnnouncement),
    /// BlockExecutor announcement.
    #[from]
    BlockExecutorAnnouncement(BlockExecutorAnnouncement),
    /// Deploy Gossiper announcement.
    #[from]
    DeployGossiperAnnouncement(GossiperAnnouncement<Deploy>),
    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(GossiperAnnouncement<GossipedAddress>),
    /// Linear chain announcement.
    #[from]
    LinearChainAnnouncement(LinearChainAnnouncement),
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl From<ApiRequest<NodeId>> for Event {
    fn from(request: ApiRequest<NodeId>) -> Self {
        Event::ApiServer(api_server::Event::ApiRequest(request))
    }
}

impl From<NetworkRequest<NodeId, consensus::ConsensusMessage>> for Event {
    fn from(request: NetworkRequest<NodeId, consensus::ConsensusMessage>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(request: ContractRuntimeRequest) -> Event {
        Event::ContractRuntime(contract_runtime::Event::Request(request))
    }
}

impl From<ConsensusRequest> for Event {
    fn from(request: ConsensusRequest) -> Self {
        Event::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl From<LinearChainRequest<NodeId>> for Event {
    fn from(request: LinearChainRequest<NodeId>) -> Self {
        Event::LinearChain(linear_chain::Event::Request(request))
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::DeployBuffer(event) => write!(f, "deploy buffer: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::ApiServer(event) => write!(f, "api server: {}", event),
            Event::ChainspecLoader(event) => write!(f, "chainspec loader: {}", event),
            Event::Consensus(event) => write!(f, "consensus: {}", event),
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            Event::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            Event::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            Event::ContractRuntime(event) => write!(f, "contract runtime: {}", event),
            Event::BlockExecutor(event) => write!(f, "block executor: {}", event),
            Event::LinearChain(event) => write!(f, "linear-chain event {}", event),
            Event::ProtoBlockValidator(event) => write!(f, "block validator: {}", event),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::NetworkInfoRequest(req) => write!(f, "network info request: {}", req),
            Event::ChainspecLoaderRequest(req) => write!(f, "chainspec loader request: {}", req),
            Event::DeployFetcherRequest(req) => write!(f, "deploy fetcher request: {}", req),
            Event::DeployBufferRequest(req) => write!(f, "deploy buffer request: {}", req),
            Event::BlockExecutorRequest(req) => write!(f, "block executor request: {}", req),
            Event::ProtoBlockValidatorRequest(req) => write!(f, "block validator request: {}", req),
            Event::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
            Event::BlockExecutorAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            Event::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            Event::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            Event::LinearChainAnnouncement(ann) => write!(f, "linear chain announcement: {}", ann),
        }
    }
}

/// The configuration needed to initialize a Validator reactor
pub struct ValidatorInitConfig {
    pub(super) config: Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) consensus: EraSupervisor<NodeId>,
    pub(super) init_consensus_effects: Effects<consensus::Event<NodeId>>,
    pub(super) linear_chain: Vec<Block>,
    pub(super) finalized_deploys: ProtoBlockCollection,
}

/// Validator node reactor.
#[derive(DataSize, Debug)]
pub struct Reactor {
    metrics: Metrics,
    net: SmallNetwork<Event, Message>,
    address_gossiper: Gossiper<GossipedAddress, Event>,
    storage: Storage,
    contract_runtime: ContractRuntime,
    api_server: ApiServer,
    chainspec_loader: ChainspecLoader,
    consensus: EraSupervisor<NodeId>,
    #[data_size(skip)]
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, Event>,
    deploy_buffer: DeployBuffer,
    block_executor: BlockExecutor,
    proto_block_validator: BlockValidator<ProtoBlock, NodeId>,
    linear_chain: LinearChain<NodeId>,

    // Non-components.
    #[data_size(skip)] // Never allocates heap data.
    memory_metrics: MemoryMetrics,

    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
}

#[cfg(test)]
impl Reactor {
    /// Inspect consensus.
    pub(crate) fn consensus(&self) -> &EraSupervisor<NodeId> {
        &self.consensus
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    // The "configuration" is in fact the whole state of the joiner reactor, which we
    // deconstruct and reuse.
    type Config = ValidatorInitConfig;
    type Error = Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        // We don't need `rng` b/c consensus component was the only one using it,
        // and now it's being passed on from the `joiner` reactor via `config`.
        _rng: &mut dyn CryptoRngCore,
    ) -> Result<(Self, Effects<Event>), Error> {
        let ValidatorInitConfig {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
            consensus,
            init_consensus_effects,
            linear_chain,
            finalized_deploys,
        } = config;

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

        let effect_builder = EffectBuilder::new(event_queue);
        let (net, net_effects) = SmallNetwork::new(event_queue, config.network, true)?;

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let api_server = ApiServer::new(config.http_server, effect_builder);
        let deploy_acceptor = DeployAcceptor::new();
        let deploy_fetcher = Fetcher::new(config.fetcher);
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, Event>,
            registry,
        )?;
        let (deploy_buffer, deploy_buffer_effects) =
            DeployBuffer::new(registry.clone(), effect_builder, finalized_deploys)?;
        let mut effects = reactor::wrap_effects(Event::DeployBuffer, deploy_buffer_effects);
        // Post state hash is expected to be present.
        let genesis_state_root_hash = chainspec_loader
            .genesis_state_root_hash()
            .expect("should have state root hash");
        let block_executor = BlockExecutor::new(genesis_state_root_hash)
            .with_parent_map(linear_chain.last().cloned());
        let proto_block_validator = BlockValidator::new();
        let linear_chain = LinearChain::new();

        effects.extend(reactor::wrap_effects(Event::Network, net_effects));
        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            init_consensus_effects,
        ));

        // set timeout to 5 minutes after now, or 5 minutes after genesis, whichever is later
        let now = Timestamp::now();
        let five_minutes = TimeDiff::from_str("5minutes").unwrap();
        let later_timestamp = cmp::max(
            now,
            chainspec_loader
                .chainspec()
                .genesis
                .highway_config
                .genesis_era_start_timestamp,
        );
        let timer_duration = later_timestamp + five_minutes - now;
        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            effect_builder
                .set_timeout(timer_duration.into())
                .event(|_| consensus::Event::Shutdown),
        ));

        Ok((
            Reactor {
                metrics,
                net,
                address_gossiper,
                storage,
                contract_runtime,
                api_server,
                chainspec_loader,
                consensus,
                deploy_acceptor,
                deploy_fetcher,
                deploy_gossiper,
                deploy_buffer,
                block_executor,
                proto_block_validator,
                linear_chain,
                memory_metrics,
                event_queue_metrics,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut dyn CryptoRngCore,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.net.handle_event(effect_builder, rng, event),
            ),
            Event::DeployBuffer(event) => reactor::wrap_effects(
                Event::DeployBuffer,
                self.deploy_buffer.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::ApiServer(event) => reactor::wrap_effects(
                Event::ApiServer,
                self.api_server.handle_event(effect_builder, rng, event),
            ),
            Event::ChainspecLoader(event) => reactor::wrap_effects(
                Event::ChainspecLoader,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            Event::Consensus(event) => reactor::wrap_effects(
                Event::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcher(event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::AddressGossiper(event) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockExecutor(event) => reactor::wrap_effects(
                Event::BlockExecutor,
                self.block_executor.handle_event(effect_builder, rng, event),
            ),
            Event::ProtoBlockValidator(event) => reactor::wrap_effects(
                Event::ProtoBlockValidator,
                self.proto_block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            Event::LinearChain(event) => reactor::wrap_effects(
                Event::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),

            // Requests:
            Event::NetworkRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::Network(small_network::Event::from(req)),
            ),
            Event::NetworkInfoRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::Network(small_network::Event::from(req)),
            ),
            Event::DeployFetcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(req.into()))
            }
            Event::DeployBufferRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployBuffer(req.into()))
            }
            Event::BlockExecutorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::BlockExecutor(block_executor::Event::from(req)),
            ),
            Event::ProtoBlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::ProtoBlockValidator(block_validator::Event::from(req)),
            ),
            Event::MetricsRequest(req) => reactor::wrap_effects(
                Event::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            Event::ChainspecLoaderRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::ChainspecLoader(req.into()))
            }

            // Announcements:
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let reactor_event = match payload {
                    Message::Consensus(msg) => {
                        Event::Consensus(consensus::Event::MessageReceived { sender, msg })
                    }
                    Message::DeployGossiper(message) => {
                        Event::DeployGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                    Message::AddressGossiper(message) => {
                        Event::AddressGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                    Message::GetRequest { tag, serialized_id } => match tag {
                        Tag::Deploy => {
                            let deploy_hash = match bincode::deserialize(&serialized_id) {
                                Ok(hash) => hash,
                                Err(error) => {
                                    error!(
                                        "failed to decode {:?} from {}: {}",
                                        serialized_id, sender, error
                                    );
                                    return Effects::new();
                                }
                            };
                            Event::Storage(storage::Event::GetDeployForPeer {
                                deploy_hash,
                                peer: sender,
                            })
                        }
                        Tag::Block => {
                            let block_hash = match bincode::deserialize(&serialized_id) {
                                Ok(hash) => hash,
                                Err(error) => {
                                    error!(
                                        "failed to decode {:?} from {}: {}",
                                        serialized_id, sender, error
                                    );
                                    return Effects::new();
                                }
                            };
                            Event::LinearChain(linear_chain::Event::Request(
                                LinearChainRequest::BlockRequest(block_hash, sender),
                            ))
                        }
                        Tag::BlockByHeight => {
                            let height = match bincode::deserialize(&serialized_id) {
                                Ok(block_by_height) => block_by_height,
                                Err(error) => {
                                    error!(
                                        "failed to decode {:?} from {}: {}",
                                        serialized_id, sender, error
                                    );
                                    return Effects::new();
                                }
                            };
                            Event::LinearChain(linear_chain::Event::Request(
                                LinearChainRequest::BlockAtHeight(height, sender),
                            ))
                        }
                        Tag::GossipedAddress => {
                            warn!("received get request for gossiped-address from {}", sender);
                            return Effects::new();
                        }
                    },
                    Message::GetResponse {
                        tag,
                        serialized_item,
                    } => match tag {
                        Tag::Deploy => {
                            let deploy = match bincode::deserialize(&serialized_item) {
                                Ok(deploy) => Box::new(deploy),
                                Err(error) => {
                                    error!("failed to decode deploy from {}: {}", sender, error);
                                    return Effects::new();
                                }
                            };
                            Event::DeployAcceptor(deploy_acceptor::Event::Accept {
                                deploy,
                                source: Source::Peer(sender),
                            })
                        }
                        Tag::Block => todo!("Handle GET block response"),
                        Tag::BlockByHeight => todo!("Handle GET BlockByHeight response"),
                        Tag::GossipedAddress => {
                            warn!("received get request for gossiped-address from {}", sender);
                            return Effects::new();
                        }
                    },
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(gossiped_address)) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::AddressGossiper(event))
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(peer_id)) => {
                debug!(%peer_id, "new peer announcement event ignored (validator reactor does not care)");
                Effects::new()
            }
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
                deploy,
                source,
            }) => {
                let event = deploy_buffer::Event::Buffer {
                    hash: *deploy.id(),
                    header: Box::new(deploy.header().clone()),
                };
                let mut effects =
                    self.dispatch_event(effect_builder, rng, Event::DeployBuffer(event));

                let event = gossiper::Event::ItemReceived {
                    item_id: *deploy.id(),
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::DeployGossiper(event),
                ));

                let event = fetcher::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::DeployFetcher(event),
                ));

                effects
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            Event::ConsensusAnnouncement(consensus_announcement) => {
                let mut reactor_event_dispatch = |dbe: deploy_buffer::Event| {
                    self.dispatch_event(effect_builder, rng, Event::DeployBuffer(dbe))
                };

                match consensus_announcement {
                    ConsensusAnnouncement::Proposed(block) => {
                        reactor_event_dispatch(deploy_buffer::Event::ProposedProtoBlock(block))
                    }
                    ConsensusAnnouncement::Finalized(block) => {
                        let mut effects = reactor_event_dispatch(
                            deploy_buffer::Event::FinalizedProtoBlock(block.proto_block().clone()),
                        );
                        let reactor_event =
                            Event::ApiServer(api_server::Event::BlockFinalized(block));
                        effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                        effects
                    }
                    ConsensusAnnouncement::Orphaned(block) => {
                        reactor_event_dispatch(deploy_buffer::Event::OrphanedProtoBlock(block))
                    }
                    ConsensusAnnouncement::Handled(_) => {
                        debug!("Ignoring `Handled` announcement in `validator` reactor.");
                        Effects::new()
                    }
                }
            }
            Event::BlockExecutorAnnouncement(BlockExecutorAnnouncement::LinearChainBlock {
                block,
                execution_results,
            }) => {
                let block_hash = *block.hash();
                let reactor_event = Event::LinearChain(linear_chain::Event::LinearChainBlock {
                    block: Box::new(block),
                    execution_results: execution_results
                        .iter()
                        .map(|(hash, (_header, results))| (*hash, results.clone()))
                        .collect(),
                });
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                for (deploy_hash, (deploy_header, execution_result)) in execution_results {
                    let reactor_event = Event::ApiServer(api_server::Event::DeployProcessed {
                        deploy_hash,
                        deploy_header: Box::new(deploy_header),
                        block_hash,
                        execution_result,
                    });
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                }
                effects
            }
            Event::DeployGossiperAnnouncement(_ann) => {
                unreachable!("the deploy gossiper should never make an announcement")
            }
            Event::AddressGossiperAnnouncement(ann) => {
                let GossiperAnnouncement::NewCompleteItem(gossiped_address) = ann;
                let reactor_event =
                    Event::Network(small_network::Event::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded {
                block_hash,
                block_header,
            }) => {
                let reactor_event = Event::ApiServer(api_server::Event::BlockAdded {
                    block_hash,
                    block_header,
                });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
        }
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(&self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle)
    }
}

#[cfg(test)]
impl NetworkedReactor for Reactor {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        self.net.node_id()
    }
}
