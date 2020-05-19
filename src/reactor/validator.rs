//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    components::small_network::{self, SmallNetwork},
    config::Config,
    effect::Effect,
    reactor::{self, EventQueueHandle, Scheduler},
    utils::Multiple,
};

/// Top-level event for the reactor.
#[derive(Debug)]
#[must_use]
pub enum Event {
    Network(small_network::Event<Message>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message {}

/// Validator node reactor.
pub struct Reactor {
    net: SmallNetwork<Self, Message>,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        cfg: Config,
        scheduler: &'static Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let (net, net_effects) = SmallNetwork::new(
            EventQueueHandle::bind(scheduler, Event::Network),
            cfg.validator_net,
        )?;

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
