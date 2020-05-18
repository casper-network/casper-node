//! Reactor for validator nodes.
//!
//! Validator nodes join the validator only network upon startup.
use crate::components::small_network;
use crate::util::Multiple;
use crate::{config, effect, reactor};
use serde::{Deserialize, Serialize};

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
    net: small_network::SmallNetwork<Self, Message>,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        cfg: config::Config,
        scheduler: &'static reactor::Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<effect::Effect<Self::Event>>)> {
        let (net, net_effects) = small_network::SmallNetwork::new(
            reactor::EventQueueHandle::bind(scheduler, Event::Network),
            cfg.validator_net,
        )?;

        Ok((
            Reactor { net },
            reactor::wrap_effects(Event::Network, net_effects),
        ))
    }

    fn dispatch_event(&mut self, event: Event) -> Multiple<effect::Effect<Self::Event>> {
        match event {
            Event::Network(ev) => reactor::wrap_effects(Event::Network, self.net.handle_event(ev)),
        }
    }
}
