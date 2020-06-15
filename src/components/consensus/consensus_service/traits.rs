//! WARNING:
//! All of the following structs are stopgap solutions and will be entirely rewritten or replaced
//! when we will have better understanding of the domain and reactor APIs.
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::super::consensus_protocol::{ConsensusContext, NodeId, TimerId};

// Very simple reactor effect.
#[derive(Debug)]
pub(crate) enum Effect<Ev> {
    DelayEvent(Instant, TimerId),
    NewMessage(Ev),
}

//TODO: Stopgap structs that will be replaced with actual wire models.
#[derive(Debug)]
pub(crate) struct MessageWireFormat {
    pub(crate) era_id: EraId,
    pub(crate) sender: NodeId,
    // Message is opaque to the networking layer.
    // It will be materialized in the consensus component that knows what to expect.
    pub(crate) message_content: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EraId(pub(crate) u64);

pub(crate) enum ConsensusServiceError<Ctx: ConsensusContext> {
    InvalidFormat(String),
    InternalError(Ctx::Error),
}

pub(crate) enum Event {
    IncomingMessage(MessageWireFormat),
    Timer(EraId, TimerId),
}

/// API between the reactor and consensus component.
pub(crate) trait ConsensusService {
    type Ctx: ConsensusContext;

    fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Effect<Event>>, ConsensusServiceError<Self::Ctx>>;
}
