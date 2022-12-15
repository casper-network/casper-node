//! Shutdown trigger control.
//!
//! A component that can be primed with a [`StopAtSpec`] and will monitor to the node, until it
//! detects a specific spec has been triggered. If so, it instructs the system to shut down through
//!  a [`ControlAnnouncement`].

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;

use crate::{
    effect::{
        announcements::ReactorAnnouncement, requests::TriggerShutdownRequest, EffectBuilder,
        Effects,
    },
    types::NodeRng,
};

use super::{diagnostics_port::StopAtSpec, Component};

//// Shutdown trigger component.
struct ShutdownTrigger {
    /// The currently active spec for shutdown triggers.
    active_spec: Option<StopAtSpec>,
    /// The highest block height seen, if any.
    ///
    /// Constantly kept up to date, so that requests for shutting down on `block:next` can be
    /// answered without additional requests.
    highest_block_height_seen: Option<u64>,
}

/// The shutdown trigger component's event.
#[derive(DataSize, Debug, From, Serialize)]
enum Event {
    /// A reactor announcement (usually checked for block completion).
    #[from]
    ReactorAnnouncement(ReactorAnnouncement),
    /// A request to trigger a shutdown.
    #[from]
    TriggerShutdownRequest(TriggerShutdownRequest),
}

impl<REv> Component<REv> for ShutdownTrigger {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        todo!()
    }
}
