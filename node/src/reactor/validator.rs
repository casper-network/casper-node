//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;
mod memory_metrics;
#[cfg(test)]
mod tests;

use std::{
    cmp, env,
    fmt::{self, Debug, Display, Formatter},
    str::FromStr,
};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use serde::Serialize;
use tracing::{debug, error, warn};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        block_executor::{self, BlockExecutor},
        block_proposer::{self, BlockProposer},
        block_validator::{self, BlockValidator},
        chainspec_loader::{self, ChainspecLoader},
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server::{self, EventStreamServer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        metrics::Metrics,
        network::{self, Network, ENABLE_LIBP2P_ENV_VAR},
        rest_server::{self, RestServer},
        rpc_server::{self, RpcServer},
        small_network::{self, GossipedAddress, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            BlockExecutorAnnouncement, ConsensusAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, LinearChainAnnouncement, NetworkAnnouncement,
            RpcServerAnnouncement,
        },
        requests::{
            BlockExecutorRequest, BlockProposerRequest, BlockValidationRequest,
            ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
            LinearChainRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest, RestRequest,
            RpcRequest, StateStoreRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::{self, event_queue_metrics::EventQueueMetrics, EventQueueHandle},
    types::{Block, Deploy, NodeId, ProtoBlock, Tag, TimeDiff, Timestamp},
    utils::Source,
    NodeRng,
};
pub use config::Config;
pub use error::Error;
use linear_chain::LinearChain;
use memory_metrics::MemoryMetrics;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(network::Event<Message>),
    /// Small network event.
    #[from]
    SmallNetwork(small_network::Event<Message>),
    /// Block proposer event.
    #[from]
    BlockProposer(#[serde(skip_serializing)] block_proposer::Event),
    #[from]
    /// Storage event.
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    /// RPC server event.
    RpcServer(#[serde(skip_serializing)] rpc_server::Event),
    #[from]
    /// REST server event.
    RestServer(#[serde(skip_serializing)] rest_server::Event),
    #[from]
    /// Event stream server event.
    EventStreamServer(#[serde(skip_serializing)] event_stream_server::Event),
    #[from]
    /// Chainspec Loader event.
    ChainspecLoader(#[serde(skip_serializing)] chainspec_loader::Event),
    #[from]
    /// Consensus event.
    Consensus(#[serde(skip_serializing)] consensus::Event<NodeId>),
    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
    /// Deploy fetcher event.
    #[from]
    DeployFetcher(#[serde(skip_serializing)] fetcher::Event<Deploy>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(#[serde(skip_serializing)] gossiper::Event<Deploy>),
    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    /// Contract runtime event.
    #[from]
    ContractRuntime(#[serde(skip_serializing)] contract_runtime::Event),
    /// Block executor event.
    #[from]
    BlockExecutor(#[serde(skip_serializing)] block_executor::Event),
    /// Block validator event.
    #[from]
    ProtoBlockValidator(#[serde(skip_serializing)] block_validator::Event<ProtoBlock, NodeId>),
    /// Linear chain event.
    #[from]
    LinearChain(#[serde(skip_serializing)] linear_chain::Event<NodeId>),

    // Requests
    /// Network request.
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<NodeId, Message>),
    /// Network info request.
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest<NodeId>),
    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Deploy>),
    /// Block proposer request.
    #[from]
    BlockProposerRequest(#[serde(skip_serializing)] BlockProposerRequest),
    /// Block executor request.
    #[from]
    BlockExecutorRequest(#[serde(skip_serializing)] BlockExecutorRequest),
    /// Block validator request.
    #[from]
    ProtoBlockValidatorRequest(
        #[serde(skip_serializing)] BlockValidationRequest<ProtoBlock, NodeId>,
    ),
    /// Metrics request.
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),
    /// Chainspec info request
    #[from]
    ChainspecLoaderRequest(#[serde(skip_serializing)] ChainspecLoaderRequest),
    /// Storage request.
    #[from]
    StorageRequest(#[serde(skip_serializing)] StorageRequest),
    /// Request for state storage.
    #[from]
    StateStoreRequest(StateStoreRequest),

    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, Message>),
    /// API server announcement.
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),
    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(#[serde(skip_serializing)] ConsensusAnnouncement<NodeId>),
    /// BlockExecutor announcement.
    #[from]
    BlockExecutorAnnouncement(#[serde(skip_serializing)] BlockExecutorAnnouncement),
    /// Deploy Gossiper announcement.
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
    /// Linear chain announcement.
    #[from]
    LinearChainAnnouncement(#[serde(skip_serializing)] LinearChainAnnouncement),
}

impl From<RpcRequest<NodeId>> for Event {
    fn from(request: RpcRequest<NodeId>) -> Self {
        Event::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest<NodeId>> for Event {
    fn from(request: RestRequest<NodeId>) -> Self {
        Event::RestServer(rest_server::Event::RestRequest(request))
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
            Event::SmallNetwork(event) => write!(f, "small network: {}", event),
            Event::BlockProposer(event) => write!(f, "block proposer: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::RpcServer(event) => write!(f, "rpc server: {}", event),
            Event::RestServer(event) => write!(f, "rest server: {}", event),
            Event::EventStreamServer(event) => write!(f, "event stream server: {}", event),
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
            Event::StorageRequest(req) => write!(f, "storage request: {}", req),
            Event::StateStoreRequest(req) => write!(f, "state store request: {}", req),
            Event::DeployFetcherRequest(req) => write!(f, "deploy fetcher request: {}", req),
            Event::BlockProposerRequest(req) => write!(f, "block proposer request: {}", req),
            Event::BlockExecutorRequest(req) => write!(f, "block executor request: {}", req),
            Event::ProtoBlockValidatorRequest(req) => write!(f, "block validator request: {}", req),
            Event::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::RpcServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
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
    pub(super) latest_block: Option<Block>,
    pub(super) event_stream_server: EventStreamServer,
}

/// Validator node reactor.
#[derive(DataSize, Debug)]
pub struct Reactor {
    metrics: Metrics,
    small_network: SmallNetwork<Event, Message>,
    network: Network<Event, Message>,
    address_gossiper: Gossiper<GossipedAddress, Event>,
    storage: Storage,
    contract_runtime: ContractRuntime,
    rpc_server: RpcServer,
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    chainspec_loader: ChainspecLoader,
    consensus: EraSupervisor<NodeId>,
    #[data_size(skip)]
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, Event>,
    block_proposer: BlockProposer,
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
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Event>), Error> {
        let ValidatorInitConfig {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
            consensus,
            init_consensus_effects,
            latest_block,
            event_stream_server,
        } = config;

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

        let effect_builder = EffectBuilder::new(event_queue);
        let network_config = network::Config::from(&config.network);
        let (network, network_effects) = Network::new(
            event_queue,
            network_config,
            chainspec_loader.chainspec(),
            true,
        )?;
        let genesis_config_hash = chainspec_loader.chainspec().hash();
        let (small_network, small_network_effects) =
            SmallNetwork::new(event_queue, config.network, genesis_config_hash, true)?;

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let rpc_server = RpcServer::new(config.rpc_server.clone(), effect_builder);
        let rest_server = RestServer::new(config.rest_server.clone(), effect_builder);

        let deploy_acceptor = DeployAcceptor::new(config.deploy_acceptor);
        let deploy_fetcher = Fetcher::new(config.fetcher);
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, Event>,
            registry,
        )?;
        let (block_proposer, block_proposer_effects) = BlockProposer::new(
            registry.clone(),
            effect_builder,
            latest_block
                .as_ref()
                .map(|block| block.height() + 1)
                .unwrap_or(0),
        )?;
        let mut effects = reactor::wrap_effects(Event::BlockProposer, block_proposer_effects);
        // Post state hash is expected to be present.
        let genesis_state_root_hash = chainspec_loader
            .genesis_state_root_hash()
            .expect("should have state root hash");
        let block_executor = BlockExecutor::new(genesis_state_root_hash, registry.clone())
            .with_parent_map(latest_block);
        let proto_block_validator = BlockValidator::new();
        let linear_chain = LinearChain::new();

        effects.extend(reactor::wrap_effects(Event::Network, network_effects));
        effects.extend(reactor::wrap_effects(
            Event::SmallNetwork,
            small_network_effects,
        ));
        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            init_consensus_effects,
        ));

        // set timeout to 5 minutes after now, or 5 minutes after genesis, whichever is later
        let now = Timestamp::now();
        let five_minutes = TimeDiff::from_str("5minutes").unwrap();
        let later_timestamp = cmp::max(now, chainspec_loader.chainspec().genesis.timestamp);
        let timer_duration = later_timestamp + five_minutes - now;
        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            effect_builder
                .set_timeout(timer_duration.into())
                .event(|_| consensus::Event::Shutdown),
        ));

        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            effect_builder
                .immediately()
                .event(move |_| consensus::Event::FinishedJoining(now)),
        ));

        Ok((
            Reactor {
                metrics,
                network,
                small_network,
                address_gossiper,
                storage,
                contract_runtime,
                rpc_server,
                rest_server,
                event_stream_server,
                chainspec_loader,
                consensus,
                deploy_acceptor,
                deploy_fetcher,
                deploy_gossiper,
                block_proposer,
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
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::SmallNetwork(event) => reactor::wrap_effects(
                Event::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            Event::BlockProposer(event) => reactor::wrap_effects(
                Event::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::RpcServer(event) => reactor::wrap_effects(
                Event::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            Event::RestServer(event) => reactor::wrap_effects(
                Event::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            Event::EventStreamServer(event) => reactor::wrap_effects(
                Event::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
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
            Event::NetworkRequest(req) => {
                let event = if env::var(ENABLE_LIBP2P_ENV_VAR).is_ok() {
                    Event::Network(network::Event::from(req))
                } else {
                    Event::SmallNetwork(small_network::Event::from(req))
                };
                self.dispatch_event(effect_builder, rng, event)
            }
            Event::NetworkInfoRequest(req) => {
                let event = if env::var(ENABLE_LIBP2P_ENV_VAR).is_ok() {
                    Event::Network(network::Event::from(req))
                } else {
                    Event::SmallNetwork(small_network::Event::from(req))
                };
                self.dispatch_event(effect_builder, rng, event)
            }
            Event::DeployFetcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(req.into()))
            }
            Event::BlockProposerRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::BlockProposer(req.into()))
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
            Event::StorageRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::Storage(req.into()))
            }
            Event::StateStoreRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::Storage(req.into()))
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

                            match self
                                .storage
                                .handle_legacy_direct_deploy_request(deploy_hash)
                            {
                                // This functionality was moved out of the storage component and
                                // should be refactored ASAP.
                                Some(deploy) => {
                                    match Message::new_get_response(&deploy) {
                                        Ok(message) => {
                                            return effect_builder
                                                .send_message(sender, message)
                                                .ignore();
                                        }
                                        Err(error) => {
                                            error!("failed to create get-response: {}", error);
                                            return Effects::new();
                                        }
                                    };
                                }
                                None => {
                                    debug!("failed to get {} for {}", deploy_hash, sender);
                                    return Effects::new();
                                }
                            }
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
                    Message::FinalitySignature(fs) => Event::LinearChain(fs.into()),
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
            Event::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived { deploy }) => {
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
                let deploy_type = match deploy.deploy_type() {
                    Ok(deploy_type) => deploy_type,
                    Err(error) => {
                        tracing::error!("Invalid deploy: {:?}", error);
                        return Effects::new();
                    }
                };

                let event = block_proposer::Event::BufferDeploy {
                    hash: *deploy.id(),
                    deploy_type: Box::new(deploy_type),
                };
                let mut effects =
                    self.dispatch_event(effect_builder, rng, Event::BlockProposer(event));

                let event = gossiper::Event::ItemReceived {
                    item_id: *deploy.id(),
                    source: source.clone(),
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
                let mut reactor_event_dispatch = |dbe: block_proposer::Event| {
                    self.dispatch_event(effect_builder, rng, Event::BlockProposer(dbe))
                };

                match consensus_announcement {
                    ConsensusAnnouncement::Finalized(block) => {
                        let mut effects =
                            reactor_event_dispatch(block_proposer::Event::FinalizedProtoBlock {
                                block: block.proto_block().clone(),
                                height: block.height(),
                            });
                        let reactor_event = Event::EventStreamServer(
                            event_stream_server::Event::BlockFinalized(block),
                        );
                        effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                        effects
                    }
                    ConsensusAnnouncement::Handled(_) => {
                        debug!("Ignoring `Handled` announcement in `validator` reactor.");
                        Effects::new()
                    }
                    ConsensusAnnouncement::Fault {
                        era_id,
                        public_key,
                        timestamp,
                    } => {
                        let reactor_event =
                            Event::EventStreamServer(event_stream_server::Event::Fault {
                                era_id,
                                public_key: *public_key,
                                timestamp,
                            });
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::DisconnectFromPeer(_peer) => {
                        // TODO: handle the announcement and acutally disconnect
                        warn!("Disconnecting from a given peer not yet implemented.");
                        Effects::new()
                    }
                }
            }
            Event::BlockExecutorAnnouncement(BlockExecutorAnnouncement::LinearChainBlock {
                block,
                execution_results,
            }) => {
                let mut effects = Effects::new();
                let block_hash = *block.hash();

                // send to linear chain
                let reactor_event = Event::LinearChain(linear_chain::Event::LinearChainBlock {
                    block: Box::new(block),
                    execution_results: execution_results
                        .iter()
                        .map(|(hash, (_header, results))| (*hash, results.clone()))
                        .collect(),
                });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, (deploy_header, execution_result)) in execution_results {
                    let reactor_event =
                        Event::EventStreamServer(event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
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
                let reactor_event = Event::SmallNetwork(small_network::Event::PeerAddressReceived(
                    gossiped_address,
                ));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded {
                block_hash,
                block_header,
            }) => {
                let reactor_event =
                    Event::EventStreamServer(event_stream_server::Event::BlockAdded {
                        block_hash,
                        block_header,
                    });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::LinearChainAnnouncement(LinearChainAnnouncement::NewFinalitySignature(fs)) => {
                let reactor_event =
                    Event::EventStreamServer(event_stream_server::Event::FinalitySignature(fs));
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
        if env::var(ENABLE_LIBP2P_ENV_VAR).is_ok() {
            self.network.node_id()
        } else {
            self.small_network.node_id()
        }
    }
}
