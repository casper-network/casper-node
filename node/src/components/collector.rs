use std::{
    collections::HashSet,
    convert::Infallible,
    fmt::{Debug, Display},
    hash::Hash,
};

use derive_more::From;
use serde::Serialize;
use tracing::debug;

use crate::{
    effect::{announcements::MessageReceivedAnnouncement, EffectBuilder, Effects},
    types::NodeId,
    NodeRng,
};

use super::Component;

/// A network payload collector.
///
/// Stores each received payload.
#[derive(Debug)]
pub struct Collector<P: Collectable> {
    pub payloads: HashSet<P::CollectedType>,
}

impl<P: Collectable> Collector<P> {
    /// Creates a new collector.
    pub fn new() -> Self {
        Collector {
            payloads: HashSet::new(),
        }
    }
}

/// Collector event.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<P> {
    #[from]
    NetworkAnnouncement(MessageReceivedAnnouncement<NodeId, P>),
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
    P: Display + Debug + Collectable,
{
    type Event = Event<P>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        #[allow(clippy::single_match)]
        match event {
            Event::NetworkAnnouncement(MessageReceivedAnnouncement { payload, .. }) => {
                debug!("collected {}", payload);
                self.payloads.insert(payload.into_collectable());
            }
        }
        Effects::new()
    }
}

/// Collectable item trait.
///
/// Some items may be collected not by themselves, but in a modified form (e.g. hash only).
pub trait Collectable {
    type CollectedType: Eq + Hash;

    /// Transforms the item into the ultimately collected item.
    fn into_collectable(self) -> Self::CollectedType;
}
