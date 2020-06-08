//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::{
        api_server::{self, ApiServer},
        consensus::{self, Consensus},
        pinger::{self, Pinger},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::NetworkAnnouncement,
        requests::{NetworkRequest, StorageRequest},
        Effect, EffectBuilder, Multiple,
    },
    reactor::{self, EventQueueHandle},
    small_network::{self, NodeId},
    Config, SmallNetwork,
};

#[derive(Debug, Clone, From, Serialize, Deserialize)]
enum Message {
    #[from]
    Pinger(pinger::Message),
    #[from]
    Consensus(consensus::Message),
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Pinger(pinger) => write!(f, "Pinger::{}", pinger),
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
        }
    }
}

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
enum Event {
    #[from]
    Network(small_network::Event<Message>),
    #[from]
    Pinger(pinger::Event),
    #[from]
    Storage(Box<StorageRequest<Storage>>),
    #[from]
    ApiServer(api_server::Event),
    #[from]
    StorageConsumer(Box<storage::dummy::Event>),
    #[from]
    Consensus(consensus::Event),

    // Requests
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),

    // Announcements
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
}

impl From<NetworkRequest<NodeId, consensus::Message>> for Event {
    fn from(request: NetworkRequest<NodeId, consensus::Message>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, pinger::Message>> for Event {
    fn from(request: NetworkRequest<NodeId, pinger::Message>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

/// Validator node reactor.
struct Reactor {
    net: SmallNetwork<Event, Message>,
    pinger: Pinger,
    storage: Storage,
    api_server: ApiServer,
    consensus: Consensus,
    dummy_storage_consumer: storage::dummy::StorageConsumer,
    rng: ChaCha20Rng,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        cfg: Config,
        eq: EventQueueHandle<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let eb = EffectBuilder::new(eq);
        let (net, net_effects) = SmallNetwork::new(eq, cfg)?;

        let (pinger, pinger_effects) = Pinger::new(eb);

        let storage = Storage::new();
        let (dummy_storage_consumer, storage_consumer_effects) =
            storage::dummy::StorageConsumer::new(eb);

        let (api_server, api_server_effects) = ApiServer::new(eb);
        let (consensus, consensus_effects) = Consensus::new(eb);

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(Event::Pinger, pinger_effects));
        effects.extend(reactor::wrap_effects(
            |event| Event::StorageConsumer(Box::new(event)),
            storage_consumer_effects,
        ));
        effects.extend(reactor::wrap_effects(Event::ApiServer, api_server_effects));
        effects.extend(reactor::wrap_effects(Event::Consensus, consensus_effects));

        let rng = ChaCha20Rng::from_entropy();

        Ok((
            Reactor {
                net,
                pinger,
                storage,
                api_server,
                consensus,
                dummy_storage_consumer,
                rng,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        eb: EffectBuilder<Self::Event>,
        event: Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Network(ev) => {
                reactor::wrap_effects(Event::Network, self.net.handle_event(eb, &mut self.rng, ev))
            }
            Event::Pinger(ev) => reactor::wrap_effects(
                Event::Pinger,
                self.pinger.handle_event(eb, &mut self.rng, ev),
            ),
            Event::Storage(ev) => reactor::wrap_effects(
                |event| Event::Storage(Box::new(event)),
                self.storage.handle_event(eb, &mut self.rng, *ev),
            ),
            Event::StorageConsumer(ev) => reactor::wrap_effects(
                |event| Event::StorageConsumer(Box::new(event)),
                self.dummy_storage_consumer
                    .handle_event(eb, &mut self.rng, *ev),
            ),
            Event::ApiServer(ev) => reactor::wrap_effects(
                Event::ApiServer,
                self.api_server.handle_event(eb, &mut self.rng, ev),
            ),
            Event::Consensus(ev) => reactor::wrap_effects(
                Event::Consensus,
                self.consensus.handle_event(eb, &mut self.rng, ev),
            ),

            // Requests:
            Event::NetworkRequest(req) => {
                self.dispatch_event(eb, Event::Network(small_network::Event::from(req)))
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
                };

                // Any incoming message is one for the pinger.
                self.dispatch_event(eb, reactor_event)
            }
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(ev) => write!(f, "network: {}", ev),
            Event::Pinger(ev) => write!(f, "pinger: {}", ev),
            Event::Storage(ev) => write!(f, "storage: {}", ev),
            Event::StorageConsumer(ev) => write!(f, "storage_consumer: {}", ev),
            Event::ApiServer(ev) => write!(f, "api server: {}", ev),
            Event::Consensus(ev) => write!(f, "consensus: {}", ev),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
        }
    }
}

/// Runs a validator reactor.
///
/// Starts the reactor and associated background tasks, then enters main the event processing loop.
///
/// `launch` will leak memory on start for global structures each time it is called.
///
/// Errors are returned only if component initialization fails.
pub async fn launch(cfg: Config) -> anyhow::Result<()> {
    super::launch::<Reactor>(cfg).await
}
