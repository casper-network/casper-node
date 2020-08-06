//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use hex_fmt::HexFmt;
use prometheus::Registry;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::error;

use casperlabs_types::U512;

use crate::{
    components::{
        api_server::{self, ApiServer},
        block_executor::{self, BlockExecutor},
        block_validator::{self, BlockValidator},
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor::{self, DeployAcceptor},
        deploy_buffer::{self, DeployBuffer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        metrics::Metrics,
        pinger::{self, Pinger},
        storage::{self, Storage, Value},
        Component,
    },
    effect::{
        announcements::{
            ApiServerAnnouncement, ConsensusAnnouncement, DeployAcceptorAnnouncement,
            NetworkAnnouncement,
        },
        requests::{
            ApiRequest, BlockExecutorRequest, BlockValidationRequest, ContractRuntimeRequest,
            DeployBufferRequest, FetcherRequest, MetricsRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    reactor::{self, initializer, EventQueueHandle},
    small_network::{self, NodeId},
    types::{Deploy, Item, Tag, Timestamp},
    utils::Source,
    SmallNetwork,
};
pub use config::Config;
use error::Error;

/// Reactor message.
#[derive(Debug, Clone, From, Serialize, Deserialize)]
pub enum Message {
    /// Pinger component message.
    #[from]
    Pinger(pinger::Message),
    /// Consensus component message.
    #[from]
    Consensus(consensus::ConsensusMessage),
    /// Deploy gossiper component message.
    #[from]
    DeployGossiper(gossiper::Message<Deploy>),
    /// Request to get an item from a peer.
    GetRequest {
        /// The type tag of the requested item.
        tag: Tag,
        /// The serialized ID of the requested item.
        serialized_id: Vec<u8>,
    },
    /// Response to a `GetRequest`.
    GetResponse {
        /// The type tag of the contained item.
        tag: Tag,
        /// The serialized item.
        serialized_item: Vec<u8>,
    },
}

impl Message {
    pub(crate) fn new_get_request<T: Item>(id: &T::Id) -> Result<Self, Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: bincode::serialize(id)
                .map_err(|error| Error::from_serialization(*error))?,
        })
    }

    pub(crate) fn new_get_response<T: Item>(item: &T) -> Result<Self, Error> {
        Ok(Message::GetResponse {
            tag: T::TAG,
            serialized_item: bincode::serialize(item)
                .map_err(|error| Error::from_serialization(*error))?,
        })
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Pinger(pinger) => write!(f, "Pinger::{}", pinger),
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
            Message::DeployGossiper(deploy) => write!(f, "DeployGossiper::{}", deploy),
            Message::GetRequest { tag, serialized_id } => {
                write!(f, "GetRequest({}-{:10})", tag, HexFmt(serialized_id))
            }
            Message::GetResponse {
                tag,
                serialized_item,
            } => write!(f, "GetResponse({}-{:10})", tag, HexFmt(serialized_item)),
        }
    }
}

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
    /// Pinger event.
    #[from]
    Pinger(pinger::Event),
    #[from]
    /// Storage event.
    Storage(storage::Event<Storage>),
    #[from]
    /// API server event.
    ApiServer(api_server::Event),
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
    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),
    /// Block executor event.
    #[from]
    BlockExecutor(block_executor::Event),
    /// Block validator event.
    #[from]
    BlockValidator(block_validator::Event<NodeId>),

    // Requests
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
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
    BlockValidatorRequest(BlockValidationRequest<NodeId>),
    /// Metrics request.
    #[from]
    MetricsRequest(MetricsRequest),

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
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl From<ApiRequest> for Event {
    fn from(request: ApiRequest) -> Self {
        Event::ApiServer(api_server::Event::ApiRequest(request))
    }
}

impl From<NetworkRequest<NodeId, consensus::ConsensusMessage>> for Event {
    fn from(request: NetworkRequest<NodeId, consensus::ConsensusMessage>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, pinger::Message>> for Event {
    fn from(request: NetworkRequest<NodeId, pinger::Message>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(request: ContractRuntimeRequest) -> Event {
        Event::ContractRuntime(contract_runtime::Event::Request(request))
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::DeployBuffer(event) => write!(f, "deploy buffer: {}", event),
            Event::Pinger(event) => write!(f, "pinger: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::ApiServer(event) => write!(f, "api server: {}", event),
            Event::Consensus(event) => write!(f, "consensus: {}", event),
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            Event::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            Event::ContractRuntime(event) => write!(f, "contract runtime: {}", event),
            Event::BlockExecutor(event) => write!(f, "block executor: {}", event),
            Event::BlockValidator(event) => write!(f, "block validator: {}", event),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::DeployFetcherRequest(req) => write!(f, "deploy fetcher request: {}", req),
            Event::DeployBufferRequest(req) => write!(f, "deploy buffer request: {}", req),
            Event::BlockExecutorRequest(req) => write!(f, "block executor request: {}", req),
            Event::BlockValidatorRequest(req) => write!(f, "block validator request: {}", req),
            Event::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
        }
    }
}

/// Validator node reactor.
#[derive(Debug)]
pub struct Reactor {
    metrics: Metrics,
    net: SmallNetwork<Event, Message>,
    pinger: Pinger,
    storage: Storage,
    contract_runtime: ContractRuntime,
    api_server: ApiServer,
    consensus: EraSupervisor<NodeId>,
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, Event>,
    deploy_buffer: DeployBuffer,
    block_executor: BlockExecutor,
    block_validator: BlockValidator<NodeId>,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = initializer::Reactor;
    type Error = Error;

    fn new<R: Rng + ?Sized>(
        initializer: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut R,
    ) -> Result<(Self, Effects<Event>), Error> {
        let initializer::Reactor {
            config,
            chainspec_handler,
            storage,
            contract_runtime,
        } = initializer;

        let metrics = Metrics::new(registry.clone());

        let effect_builder = EffectBuilder::new(event_queue);
        let (net, net_effects) = SmallNetwork::new(event_queue, config.validator_net)?;

        let (pinger, pinger_effects) = Pinger::new(registry, effect_builder)?;
        let api_server = ApiServer::new(config.http_server, effect_builder);
        let timestamp = Timestamp::now();
        let validator_stakes = chainspec_handler
            .chainspec()
            .genesis
            .accounts
            .iter()
            .filter_map(|account| {
                account
                    .public_key()
                    .map(|pub_key| (pub_key, account.bonded_amount()))
            })
            .filter(|(_, stake)| stake.value() > U512::zero())
            .collect();
        let (consensus, consensus_effects) = EraSupervisor::new(
            timestamp,
            config.consensus,
            effect_builder,
            validator_stakes,
        )?;
        let deploy_acceptor = DeployAcceptor::new();
        let deploy_fetcher = Fetcher::new(config.gossip);
        let deploy_gossiper = Gossiper::new(config.gossip, gossiper::get_deploy_from_storage);
        let deploy_buffer = DeployBuffer::new(config.node.block_max_deploy_count as usize);
        // Post state hash is expected to be present
        let post_state_hash = chainspec_handler
            .post_state_hash()
            .expect("should have post state hash");
        let block_executor = BlockExecutor::new(post_state_hash);
        let block_validator = BlockValidator::<NodeId>::new();

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(Event::Pinger, pinger_effects));
        effects.extend(reactor::wrap_effects(Event::Consensus, consensus_effects));

        Ok((
            Reactor {
                metrics,
                net,
                pinger,
                storage,
                contract_runtime,
                api_server,
                consensus,
                deploy_acceptor,
                deploy_fetcher,
                deploy_gossiper,
                deploy_buffer,
                block_executor,
                block_validator,
            },
            effects,
        ))
    }

    fn dispatch_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
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
            Event::Pinger(event) => reactor::wrap_effects(
                Event::Pinger,
                self.pinger.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::ApiServer(event) => reactor::wrap_effects(
                Event::ApiServer,
                self.api_server.handle_event(effect_builder, rng, event),
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
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockExecutor(event) => reactor::wrap_effects(
                Event::BlockExecutor,
                self.block_executor.handle_event(effect_builder, rng, event),
            ),
            Event::BlockValidator(event) => reactor::wrap_effects(
                Event::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),

            // Requests:
            Event::NetworkRequest(req) => self.dispatch_event(
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
            Event::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::BlockValidator(block_validator::Event::from(req)),
            ),
            Event::MetricsRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::MetricsRequest(req))
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
                    Message::Pinger(msg) => {
                        Event::Pinger(pinger::Event::MessageReceived { sender, msg })
                    }
                    Message::DeployGossiper(message) => {
                        Event::DeployGossiper(gossiper::Event::MessageReceived { sender, message })
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
                    },
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
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
                let reactor_event = Event::DeployBuffer(match consensus_announcement {
                    ConsensusAnnouncement::Proposed(block) => {
                        deploy_buffer::Event::ProposedProtoBlock(block)
                    }
                    ConsensusAnnouncement::Finalized(block) => {
                        deploy_buffer::Event::FinalizedProtoBlock(block)
                    }
                    ConsensusAnnouncement::Orphaned(block) => {
                        deploy_buffer::Event::OrphanedProtoBlock(block)
                    }
                });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
        }
    }
}
