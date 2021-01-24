use std::{
    convert::Infallible,
    fmt::{Debug, Display},
};

use derive_more::From;
use serde::Serialize;
use tracing::info;

use crate::{
    effect::{announcements::NetworkAnnouncement, EffectBuilder, Effects},
    types::NodeId,
    NodeRng,
};

use super::Component;

/// A network payload collector.
///
/// Stores each received payload.
#[derive(Debug)]
pub struct Collector<P> {
    pub payloads: Vec<P>,
}

impl<P> Collector<P> {
    /// Creates a new collector.
    pub fn new() -> Self {
        Collector {
            payloads: Vec::new(),
        }
    }
}

/// Collector event.
#[derive(Debug, From, Serialize)]
pub enum Event<P> {
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, P>),
}

impl<P> Display for Event<P>
where
    P: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl<REv, P> Component<REv> for Collector<P>
where
    P: Display,
{
    type Event = Event<P>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                payload, ..
            }) => {
                info!("collected {}", payload);
                self.payloads.push(payload);
            }
            _ => {}
        }
        Effects::new()
    }
}
