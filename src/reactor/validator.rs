//! Reactor for validator nodes.
//!
//! Validator nodes join the validator only network upon startup.
use crate::util::Multiple;
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
        _sched: &'static reactor::Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<effect::Effect<Self::Event>>)> {
        // TODO: Instantiate components here.
        let mut _effects = Multiple::new();

        Ok((Reactor, _effects))
    }

    fn dispatch_event(&mut self, _event: Event) -> Multiple<effect::Effect<Self::Event>> {
        todo!()
    }
}
