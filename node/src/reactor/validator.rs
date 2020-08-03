//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use prometheus::Registry;
use rand::Rng;
use serde::{Deserialize, Serialize};

use casperlabs_types::U512;

use crate::{
    components::{
        api_server::{self, ApiServer},
        block_executor::{self, BlockExecutor},
        block_validator::{self, BlockValidator},
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_buffer::{self, DeployBuffer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        metrics::Metrics,
        pinger::{self, Pinger},
        storage::Storage,
        Component,
    },
    effect::{
        announcements::{
            ApiServerAnnouncement, ConsensusAnnouncement, NetworkAnnouncement, StorageAnnouncement,
        },
        requests::{
            ApiRequest, BlockExecutorRequest, BlockValidationRequest, ContractRuntimeRequest,
            DeployBufferRequest, FetcherRequest, MetricsRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    reactor::{self, initializer, EventQueueHandle},
    small_network::{self, NodeId},
    types::{Deploy, Timestamp},
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
    /// Deploy fetcher component message.
    #[from]
    DeployFetcher(fetcher::Message<Deploy>),
    /// Deploy gossiper component message.
    #[from]
    DeployGossiper(gossiper::Message<Deploy>),
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Pinger(pinger) => write!(f, "Pinger::{}", pinger),
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
            Message::DeployFetcher(message) => write!(f, "DeployFetcher::{}", message),
            Message::DeployGossiper(deploy) => write!(f, "DeployGossiper::{}", deploy),
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
    Storage(StorageRequest<Storage>),
    #[from]
    /// API server event.
    ApiServer(api_server::Event),
    #[from]
    /// Consensus event.
    Consensus(consensus::Event<NodeId>),
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
    /// Storage announcement.
    #[from]
    StorageAnnouncement(StorageAnnouncement<Storage>),
    /// API server announcement.
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(ConsensusAnnouncement),
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

impl From<NetworkRequest<NodeId, fetcher::Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<NodeId, fetcher::Message<Deploy>>) -> Self {
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
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
            Event::StorageAnnouncement(ann) => write!(f, "storage announcement: {}", ann),
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
            .filter_map(|account| account.public_key().map(|pk| (pk, account.bonded_amount())))
            .filter(|(_, stake)| stake.value() > U512::zero())
            .collect();
        let (consensus, consensus_effects) = EraSupervisor::new(
            timestamp,
            config.consensus,
            effect_builder,
            validator_stakes,
        )?;
        let deploy_fetcher = Fetcher::new(config.gossip);
        let deploy_gossiper = Gossiper::new(
            config.gossip,
            gossiper::put_deploy_to_storage,
            gossiper::get_deploy_from_storage,
        );
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
                api_server,
                consensus,
                deploy_fetcher,
                deploy_gossiper,
                contract_runtime,
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
                    Message::DeployFetcher(payload) => {
                        Event::DeployFetcher(fetcher::Event::MessageReceived { sender, payload })
                    }
                    Message::DeployGossiper(message) => {
                        Event::DeployGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = gossiper::Event::ItemReceived { item: deploy };
                self.dispatch_event(effect_builder, rng, Event::DeployGossiper(event))
            }
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
            Event::StorageAnnouncement(StorageAnnouncement::StoredNewDeploy {
                deploy_hash,
                deploy_header,
            }) => {
                let event = deploy_buffer::Event::Buffer {
                    hash: deploy_hash,
                    header: deploy_header,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployBuffer(event))
            }
        }
    }
}
