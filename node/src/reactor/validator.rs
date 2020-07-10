//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use tracing::Span;

use crate::{
    components::{
        api_server::{self, ApiServer},
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_gossiper::{self, DeployGossiper},
        pinger::{self, Pinger},
        storage::{Storage, StorageType},
        Component,
    },
    effect::{
        announcements::{ApiServerAnnouncement, NetworkAnnouncement},
        requests::{ApiRequest, NetworkRequest, StorageRequest},
        EffectBuilder, Effects,
    },
    reactor::{self, initializer, EventQueueHandle},
    small_network::{self, NodeId},
    types::Timestamp,
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
    DeployGossiper(deploy_gossiper::Message),
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Pinger(pinger) => write!(f, "Pinger::{}", pinger),
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
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
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(deploy_gossiper::Event),
    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),

    // Requests
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),

    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    /// API server announcement.
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
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

impl From<NetworkRequest<NodeId, deploy_gossiper::Message>> for Event {
    fn from(request: NetworkRequest<NodeId, deploy_gossiper::Message>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::Pinger(event) => write!(f, "pinger: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::ApiServer(event) => write!(f, "api server: {}", event),
            Event::Consensus(event) => write!(f, "consensus: {}", event),
            Event::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            Event::ContractRuntime(event) => write!(f, "contract runtime: {}", event),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
        }
    }
}

/// Validator node reactor.
#[derive(Debug)]
pub struct Reactor {
    net: SmallNetwork<Event, Message>,
    pinger: Pinger,
    storage: Storage,
    contract_runtime: ContractRuntime,
    api_server: ApiServer,
    consensus: EraSupervisor<NodeId>,
    deploy_gossiper: DeployGossiper,
    rng: ChaCha20Rng,
}

impl Reactor {
    fn init(
        config: Config,
        event_queue: EventQueueHandle<Event>,
        span: &Span,
        storage: Storage,
        contract_runtime: ContractRuntime,
        rng: ChaCha20Rng,
    ) -> Result<(Self, Effects<Event>), Error> {
        let effect_builder = EffectBuilder::new(event_queue);
        let (net, net_effects) = SmallNetwork::new(event_queue, config.validator_net)?;
        span.record("id", &tracing::field::display(net.node_id()));

        let (pinger, pinger_effects) = Pinger::new(effect_builder);
        let api_server = ApiServer::new(config.http_server, effect_builder);
        let timestamp = Timestamp::now();
        let (consensus, consensus_effects) = EraSupervisor::new(timestamp, effect_builder);
        let deploy_gossiper = DeployGossiper::new(config.gossip);

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(Event::Pinger, pinger_effects));
        effects.extend(reactor::wrap_effects(Event::Consensus, consensus_effects));

        Ok((
            Reactor {
                net,
                pinger,
                storage,
                api_server,
                consensus,
                deploy_gossiper,
                rng,
                contract_runtime,
            },
            effects,
        ))
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = Config;
    type Error = Error;

    fn new(
        config: Self::Config,
        event_queue: EventQueueHandle<Self::Event>,
        span: &Span,
    ) -> Result<(Self, Effects<Event>), Error> {
        let storage = Storage::new(&config.storage)?;
        let contract_runtime = ContractRuntime::new(&config.storage, config.contract_runtime)?;
        let rng = ChaCha20Rng::from_entropy();
        Self::init(config, event_queue, span, storage, contract_runtime, rng)
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.net.handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::Pinger(event) => reactor::wrap_effects(
                Event::Pinger,
                self.pinger
                    .handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::ApiServer(event) => reactor::wrap_effects(
                Event::ApiServer,
                self.api_server
                    .handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::Consensus(event) => reactor::wrap_effects(
                Event::Consensus,
                self.consensus
                    .handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, &mut self.rng, event),
            ),
            Event::ContractRuntime(event) => todo!("handle contract runtime event: {:?}", event),

            // Requests:
            Event::NetworkRequest(req) => self.dispatch_event(
                effect_builder,
                Event::Network(small_network::Event::from(req)),
            ),

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
                        Event::DeployGossiper(deploy_gossiper::Event::MessageReceived {
                            sender,
                            message,
                        })
                    }
                };

                // Any incoming message is one for the pinger.
                self.dispatch_event(effect_builder, reactor_event)
            }
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = deploy_gossiper::Event::DeployReceived { deploy };
                self.dispatch_event(effect_builder, Event::DeployGossiper(event))
            }
        }
    }
}

impl reactor::ReactorExt<initializer::Reactor> for Reactor {
    fn new_from(
        event_queue: EventQueueHandle<Self::Event>,
        span: &Span,
        initializer_reactor: initializer::Reactor,
    ) -> Result<(Self, Effects<Self::Event>), Error> {
        let (config, storage, contract_runtime, rng) = initializer_reactor.destructure();
        Self::init(config, event_queue, span, storage, contract_runtime, rng)
    }
}
