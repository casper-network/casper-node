//! Shutdown trigger control.
//!
//! A component that can be primed with a [`StopAtSpec`] and will monitor the node, until it
//! detects a specific spec has been triggered. If so, it instructs the system to shut down through
//! a [`ControlAnnouncement`].

use std::{fmt::Display, mem};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use tracing::{info, trace};

use casper_types::EraId;

use crate::{
    effect::{
        announcements::ControlAnnouncement, requests::SetNodeStopRequest, EffectBuilder, EffectExt,
        Effects,
    },
    types::NodeRng,
};

use super::{diagnostics_port::StopAtSpec, Component};

#[derive(DataSize, Debug, Serialize)]
pub(crate) struct CompletedBlockInfo {
    height: u64,
    era: EraId,
    is_switch_block: bool,
}

impl CompletedBlockInfo {
    pub(crate) fn new(height: u64, era: EraId, is_switch_block: bool) -> Self {
        Self {
            height,
            era,
            is_switch_block,
        }
    }
}

/// The shutdown trigger component's event.
#[derive(DataSize, Debug, From, Serialize)]
pub(crate) enum Event {
    /// An announcement that a block has been completed.
    CompletedBlock(CompletedBlockInfo),
    /// A request to trigger a shutdown.
    #[from]
    SetNodeStopRequest(SetNodeStopRequest),
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::CompletedBlock(block_info) => {
                write!(
                    f,
                    "completed block: height {}, era {}, switch_block {}",
                    block_info.height, block_info.era, block_info.is_switch_block
                )
            }
            Event::SetNodeStopRequest(inner) => {
                write!(f, "set node stop request: {}", inner)
            }
        }
    }
}

const COMPONENT_NAME: &str = "shutdown_trigger";

/// Shutdown trigger component.
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
            Event::CompletedBlock(block_info) => {
                // We ignore every block that is older than one we already possess.
                let prev_height = self.highest_block_height_seen.unwrap_or_default();
                if block_info.height > prev_height {
                    self.highest_block_height_seen = Some(block_info.height);
                }

                // Once the updating is done, check if we need to emit shutdown announcements.
                let active_spec = if let Some(spec) = self.active_spec {
                    spec
                } else {
                    trace!("received block, but no active stop-at spec, ignoring");
                    return Effects::new();
                };

                let should_shutdown = match active_spec {
                    StopAtSpec::BlockHeight(trigger_height) => block_info.height >= trigger_height,
                    StopAtSpec::EraId(trigger_era_id) => block_info.era >= trigger_era_id,
                    StopAtSpec::Immediately => {
                        // Immediate stops are handled when the request is received.
                        false
                    }
                    StopAtSpec::NextBlock => {
                        // Any block that is newer than one we already saw is a "next" block.
                        block_info.height > prev_height
                    }
                    StopAtSpec::EndOfCurrentEra => {
                        // We require that the block we just finished is a switch block.
                        block_info.height > prev_height && block_info.is_switch_block
                    }
                };

                if should_shutdown {
                    info!(
                        block_height = block_info.height,
                        block_era = block_info.era.value(),
                        is_switch_block = block_info.is_switch_block,
                        %active_spec,
                        "shutdown triggered due to fulfilled stop-at spec"
                    );
                    effect_builder.announce_user_shutdown_request().ignore()
                } else {
                    trace!(
                        block_height = block_info.height,
                        block_era = block_info.era.value(),
                        is_switch_block = block_info.is_switch_block,
                        %active_spec,
                        "not shutting down"
                    );
                    Effects::new()
                }
            }

            Event::SetNodeStopRequest(SetNodeStopRequest {
                mut stop_at,
                responder,
            }) => {
                mem::swap(&mut self.active_spec, &mut stop_at);

                let mut effects = responder.respond(stop_at).ignore();

                // If we received an immediate shutdown request, send out the control announcement
                // directly, instead of waiting for another block.
                if matches!(self.active_spec, Some(StopAtSpec::Immediately)) {
                    effects.extend(effect_builder.announce_user_shutdown_request().ignore());
                }

                effects
            }
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
