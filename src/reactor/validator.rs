//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    components::{
        storage::{self, Storage},
        Component,
    },
    effect::{Effect, EffectBuilder, Multiple},
    reactor::{self, EventQueueHandle, QueueKind, Scheduler},
    small_network, Config, SmallNetwork,
};

/// Top-level event for the reactor.
#[derive(Debug)]
#[must_use]
enum Event {
    Network(small_network::Event<Message>),
    Storage(storage::Event<Storage>),
    StorageConsumer(storage::dummy::Event),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Message {}

/// Validator node reactor.
struct Reactor {
    net: SmallNetwork<Self, Message>,
    storage: Storage,
    dummy_storage_consumer: storage::dummy::StorageConsumer,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        cfg: Config,
        scheduler: &'static Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let (net, net_effects) =
            SmallNetwork::new(EventQueueHandle::bind(scheduler, Event::Network), cfg)?;

        let storage = Storage::new();
        let storage_effect_builder = EffectBuilder::new(
            EventQueueHandle::bind(scheduler, Event::Storage),
            QueueKind::Regular,
        );
        let (dummy_storage_consumer, storage_consumer_effects) =
            storage::dummy::StorageConsumer::new::<Self>(storage_effect_builder);

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(
            Event::StorageConsumer,
            storage_consumer_effects,
        ));

        Ok((
            Reactor {
                net,
                storage,
                dummy_storage_consumer,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        scheduler: &'static Scheduler<Self::Event>,
        event: Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Network(ev) => reactor::wrap_effects(Event::Network, self.net.handle_event(ev)),
            Event::Storage(ev) => {
                reactor::wrap_effects(Event::Storage, self.storage.handle_event(ev))
            }
            Event::StorageConsumer(ev) => {
                let storage_effect_builder = EffectBuilder::<Self, _>::new(
                    EventQueueHandle::bind(scheduler, Event::Storage),
                    QueueKind::Regular,
                );
                reactor::wrap_effects(
                    Event::StorageConsumer,
                    self.dummy_storage_consumer
                        .handle_event(storage_effect_builder, ev),
                )
            }
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(ev) => write!(f, "Network[{}]", ev),
            Event::Storage(ev) => write!(f, "Storage[{}]", ev),
            Event::StorageConsumer(ev) => write!(f, "StorageConsumer[{}]", ev),
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "TODO: MessagePayload")
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
