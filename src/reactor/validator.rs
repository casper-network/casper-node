//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    effect::{Effect, Multiple},
    reactor::{self, EventQueueHandle, Scheduler},
    small_network, SmallNetwork, SmallNetworkConfig,
};

/// Top-level event for the reactor.
#[derive(Debug)]
#[must_use]
enum Event {
    Network(small_network::Event<Message>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Message {}

/// Validator node reactor.
struct Reactor {
    net: SmallNetwork<Self, Message>,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        cfg: SmallNetworkConfig,
        scheduler: &'static Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let (net, net_effects) =
            SmallNetwork::new(EventQueueHandle::bind(scheduler, Event::Network), cfg)?;

        Ok((
            Reactor { net },
            reactor::wrap_effects(Event::Network, net_effects),
        ))
    }

    fn dispatch_event(&mut self, event: Event) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Network(ev) => reactor::wrap_effects(Event::Network, self.net.handle_event(ev)),
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(ev) => write!(f, "network: {}", ev),
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
pub async fn launch(cfg: SmallNetworkConfig) -> anyhow::Result<()> {
    super::launch::<Reactor>(cfg).await
}
