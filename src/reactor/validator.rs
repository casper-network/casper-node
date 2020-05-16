//! Reactor for validator nodes.
//!
//! Validator nodes join the validator only network upon startup.
use crate::{config, effect, reactor};

/// Top-level event for the reactor.
#[derive(Debug)]
#[must_use]
pub enum Event {}

/// Validator node reactor.
pub struct Reactor;

impl reactor::Reactor for Reactor {
    type Event = Event;

    fn new(
        _cfg: &config::Config,
        _eq: reactor::EventQueueHandle<Self::Event>,
    ) -> anyhow::Result<(Self, Vec<effect::Effect<Self::Event>>)> {
        // TODO: Instantiate components here.
        let mut _effects = Vec::new();

        Ok((Reactor, _effects))
    }

    fn dispatch_event(&mut self, _event: Event) -> effect::Effect<Self::Event> {
        todo!()
    }
}
