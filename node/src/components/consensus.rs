//! The consensus component. Provides distributed consensus among the nodes in the network.
mod config;
mod consensus_protocol;
mod era_supervisor;
mod highway_core;
mod protocols;
#[cfg(test)]
mod tests;
mod traits;

use std::fmt::{self, Debug, Display, Formatter};

use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

use crate::{
    components::{storage::Storage, Component},
    effect::{
        announcements::ConsensusAnnouncement,
        requests::{
            BlockExecutorRequest, BlockValidationRequest, DeployBufferRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, Effects,
    },
    reactor::validator::Message,
    types::{Block, ProtoBlock, Timestamp},
};
pub use config::Config;
pub(crate) use consensus_protocol::BlockContext;
pub(crate) use era_supervisor::{EraId, EraSupervisor};
use hex_fmt::HexFmt;
use traits::NodeIdT;

#[derive(Clone, Serialize, Deserialize)]
pub struct ConsensusMessage {
    era_id: EraId,
    payload: Vec<u8>,
}

impl ConsensusMessage {
    fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Consensus component event.
#[derive(Debug)]
pub enum Event<I> {
    /// An incoming network message.
    MessageReceived { sender: I, msg: ConsensusMessage },
    /// A scheduled event to be handled by a specified era
    Timer { era_id: EraId, timestamp: Timestamp },
    /// We are receiving the data we require to propose a new block
    NewProtoBlock {
        era_id: EraId,
        proto_block: ProtoBlock,
        block_context: BlockContext,
    },
    /// We are receiving the information necessary to produce finality signatures
    ExecutedBlock { era_id: EraId, block: Block },
    /// The proto-block has been validated and can now be added to the protocol state
    AcceptProtoBlock {
        era_id: EraId,
        proto_block: ProtoBlock,
    },
    /// The proto-block turned out to be invalid, we might want to accuse/punish/... the sender
    InvalidProtoBlock {
        era_id: EraId,
        sender: I,
        proto_block: ProtoBlock,
    },
}

impl Display for ConsensusMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConsensusMessage {{ era_id: {}, {:10} }}",
            self.era_id.0,
            HexFmt(self.payload())
        )
    }
}

impl Debug for ConsensusMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConsensusMessage {{ era_id: {}, {:10} }}",
            self.era_id.0,
            HexFmt(self.payload())
        )
    }
}

impl<I: Debug> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::MessageReceived { sender, msg } => write!(f, "msg from {:?}: {}", sender, msg),
            Event::Timer { era_id, timestamp } => write!(
                f,
                "timer for era {:?} scheduled for timestamp {}",
                era_id, timestamp
            ),
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => write!(
                f,
                "New proto-block for era {:?}: {:?}, {:?}",
                era_id, proto_block, block_context
            ),
            Event::ExecutedBlock { era_id, block } => write!(
                f,
                "A block has been executed for era {:?}: {:?}",
                era_id, block
            ),
            Event::AcceptProtoBlock {
                era_id,
                proto_block,
            } => write!(
                f,
                "A proto-block has been validated for era {:?}: {:?}",
                era_id, proto_block
            ),
            Event::InvalidProtoBlock {
                era_id,
                sender,
                proto_block,
            } => write!(
                f,
                "A proto-block received from {:?} turned out to be invalid for era {:?}: {:?}",
                sender, era_id, proto_block
            ),
        }
    }
}

/// A helper trait whose bounds represent the requirements for a reactor event that `EraSupervisor`
/// can work with.
pub trait ReactorEventT<I>:
    From<Event<I>>
    + Send
    + From<NetworkRequest<I, Message>>
    + From<DeployBufferRequest>
    + From<ConsensusAnnouncement>
    + From<BlockExecutorRequest>
    + From<BlockValidationRequest<I>>
    + From<StorageRequest<Storage>>
{
}

impl<REv, I> ReactorEventT<I> for REv where
    REv: From<Event<I>>
        + Send
        + From<NetworkRequest<I, Message>>
        + From<DeployBufferRequest>
        + From<ConsensusAnnouncement>
        + From<BlockExecutorRequest>
        + From<BlockValidationRequest<I>>
        + From<StorageRequest<Storage>>
{
}

impl<I, REv, R> Component<REv, R> for EraSupervisor<I, R>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event<I>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let mut handling_es = self.handling(effect_builder, rng);
        match event {
            Event::Timer { era_id, timestamp } => handling_es.handle_timer(era_id, timestamp),
            Event::MessageReceived { sender, msg } => handling_es.handle_message(sender, msg),
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => handling_es.handle_new_proto_block(era_id, proto_block, block_context),
            Event::ExecutedBlock { era_id, block } => {
                handling_es.handle_executed_block(era_id, block)
            }
            Event::AcceptProtoBlock {
                era_id,
                proto_block,
            } => handling_es.handle_accept_proto_block(era_id, proto_block),
            Event::InvalidProtoBlock {
                era_id,
                sender,
                proto_block,
            } => handling_es.handle_invalid_proto_block(era_id, sender, proto_block),
        }
    }
}
