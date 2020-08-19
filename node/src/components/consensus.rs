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

use rand::Rng;
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
    types::{Block, ProtoBlock, Timestamp},
};
pub use config::Config;
pub(crate) use consensus_protocol::BlockContext;
pub(crate) use era_supervisor::{EraId, EraSupervisor};
use traits::NodeIdT;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusMessage {
    era_id: EraId,
    payload: Vec<u8>,
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
        write!(f, "ConsensusMessage {{ era_id: {}, .. }}", self.era_id.0)
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
    + From<NetworkRequest<I, ConsensusMessage>>
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
        + From<NetworkRequest<I, ConsensusMessage>>
        + From<DeployBufferRequest>
        + From<ConsensusAnnouncement>
        + From<BlockExecutorRequest>
        + From<BlockValidationRequest<I>>
        + From<StorageRequest<Storage>>
{
}

impl<I, REv> Component<REv> for EraSupervisor<I>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
{
    type Event = Event<I>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Timer { era_id, timestamp } => {
                self.handle_timer(effect_builder, era_id, timestamp)
            }
            Event::MessageReceived { sender, msg } => {
                self.handle_message(effect_builder, sender, msg)
            }
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => self.handle_new_proto_block(effect_builder, era_id, proto_block, block_context),
            Event::ExecutedBlock { era_id, block } => {
                self.handle_executed_block(effect_builder, era_id, block)
            }
            Event::AcceptProtoBlock {
                era_id,
                proto_block,
            } => self.handle_accept_proto_block(effect_builder, era_id, proto_block),
            Event::InvalidProtoBlock {
                era_id,
                sender,
                proto_block,
            } => self.handle_invalid_proto_block(effect_builder, era_id, sender, proto_block),
        }
    }
}
