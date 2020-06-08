//! WARNING:
//! All of the following structs are stopgap solutions and will be entirely rewritten or replaced
//! when we will have better understanding of the domain and reactor APIs.
use super::super::consensus_protocol::{NodeId, TimerId};
use std::time::Instant;

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

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct EraId(u64);
