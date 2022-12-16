//! Shutdown trigger control.
//!
//! A component that can be primed with a [`StopAtSpec`] and will monitor to the node, until it
//! detects a specific spec has been triggered. If so, it instructs the system to shut down through
//!  a [`ControlAnnouncement`].

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use tracing::{info, trace};

use crate::{
    effect::{
        announcements::{ControlAnnouncement, ReactorAnnouncement},
        requests::TriggerShutdownRequest,
        EffectBuilder, EffectExt, Effects,
    },
    types::NodeRng,
};

use super::{diagnostics_port::StopAtSpec, Component};

//// Shutdown trigger component.
#[derive(DataSize, Debug)]
pub(crate) struct ShutdownTrigger {
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
pub(crate) enum Event {
    /// A reactor announcement (usually checked for block completion).
    #[from]
    ReactorAnnouncement(ReactorAnnouncement),
    /// A request to trigger a shutdown.
    #[from]
    TriggerShutdownRequest(TriggerShutdownRequest),
}

impl ShutdownTrigger {
    /// Creates a new instance of the shutdown trigger component.
    pub(crate) fn new() -> Self {
        Self {
            active_spec: None,
            highest_block_height_seen: None,
        }
    }
}

impl<REv> Component<REv> for ShutdownTrigger
where
    REv: Send + From<ControlAnnouncement>,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::ReactorAnnouncement(ReactorAnnouncement::CompletedBlock { block }) => {
                // We ignore every block that is older than one we already possess.
                let prev_height = self.highest_block_height_seen.unwrap_or_default();
                if block.height() > prev_height {
                    self.highest_block_height_seen = Some(block.height());
                }

                // Once the updating is done, check if we need to emit shutdown announcements.
                let active_spec = if let Some(spec) = self.active_spec {
                    spec
                } else {
                    trace!("received block, but no active stop-at spec, ignoring");
                    return Effects::new();
                };

                let should_shutdown = match active_spec {
                    StopAtSpec::BlockHeight(trigger_height) => block.height() >= trigger_height,
                    StopAtSpec::EraId(trigger_era_id) => block.header().era_id() >= trigger_era_id,
                    StopAtSpec::Immediately => true,
                    StopAtSpec::NextBlock => {
                        // Any block that is newer than one we already saw is a "next" block.
                        block.height() > prev_height
                    }
                    StopAtSpec::NextEra => {
                        // We require that the block we just finished is a switch block.
                        block.height() > prev_height && block.header().is_switch_block()
                    }
                };

                if should_shutdown {
                    info!(
                        block_height = block.height(),
                        block_era = block.header().era_id().value(),
                        is_switch_block = block.header().is_switch_block(),
                        %active_spec,
                        "shutdown triggered due to fulfilled stop-at spec"
                    );
                    effect_builder.announce_user_shutdown_request().ignore()
                } else {
                    trace!(
                        block_height = block.height(),
                        block_era = block.header().era_id().value(),
                        is_switch_block = block.header().is_switch_block(),
                        %active_spec,
                        "not shutting down"
                    );
                    Effects::new()
                }
            }

            Event::TriggerShutdownRequest(_) => todo!(),
        }
    }
}
