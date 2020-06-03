//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    components::{
        pinger::{self, Message, Pinger},
        storage::{self, Storage},
        Component,
    },
    effect::{
        requests::{NetworkRequest, StorageRequest},
        Effect, EffectBuilder, Multiple,
    },
    reactor::{self, EventQueueHandle},
    small_network::{self, NodeId},
    Config, SmallNetwork,
};

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
    StorageConsumer(Box<storage::dummy::Event>),
}

impl From<NetworkRequest<NodeId, Message>> for Event {
    fn from(req: NetworkRequest<NodeId, Message>) -> Self {
        Event::Network(small_network::Event::from(req))
    }
}

/// Validator node reactor.
struct Reactor {
    net: SmallNetwork<Event, Message>,
    pinger: Pinger,
    storage: Storage,
    dummy_storage_consumer: storage::dummy::StorageConsumer,
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

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(Event::Pinger, pinger_effects));
        effects.extend(reactor::wrap_effects(
            |event| Event::StorageConsumer(Box::new(event)),
            storage_consumer_effects,
        ));

        Ok((
            Reactor {
                net,
                pinger,
                storage,
                dummy_storage_consumer,
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
                reactor::wrap_effects(Event::Network, self.net.handle_event(eb, ev))
            }
            Event::Pinger(ev) => {
                reactor::wrap_effects(Event::Pinger, self.pinger.handle_event(eb, ev))
            }
            Event::Storage(ev) => reactor::wrap_effects(
                |event| Event::Storage(Box::new(event)),
                self.storage.handle_event(eb, *ev),
            ),
            Event::StorageConsumer(ev) => reactor::wrap_effects(
                |event| Event::StorageConsumer(Box::new(event)),
                self.dummy_storage_consumer.handle_event(eb, *ev),
            ),
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
