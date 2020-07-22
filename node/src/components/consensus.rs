//! The consensus component. Provides distributed consensus among the nodes in the network.
mod config;
mod consensus_protocol;
mod era_supervisor;
mod highway_core;
mod protocols;
mod traits;

#[cfg(test)]
#[allow(unused)]
#[allow(dead_code)]
mod tests;

use std::fmt::{self, Debug, Display, Formatter};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::Component,
    effect::{
        announcements::ConsensusAnnouncement,
        requests::{BlockExecutorRequest, DeployQueueRequest, NetworkRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{ExecutedBlock, ProtoBlock, Timestamp},
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
    ExecutedBlock {
        era_id: EraId,
        executed_block: ExecutedBlock,
    },
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
            Event::ExecutedBlock {
                era_id,
                executed_block,
            } => write!(
                f,
                "A block has been executed for era {:?}: {:?}",
                era_id, executed_block
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
    + From<DeployQueueRequest>
    + From<ConsensusAnnouncement>
    + From<BlockExecutorRequest>
{
}

impl<REv, I> ReactorEventT<I> for REv where
    REv: From<Event<I>>
        + Send
        + From<NetworkRequest<I, ConsensusMessage>>
        + From<DeployQueueRequest>
        + From<ConsensusAnnouncement>
        + From<BlockExecutorRequest>
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
                self.delegate_to_era(era_id, effect_builder, move |consensus| {
                    consensus.handle_timer(timestamp)
                })
            }
            Event::MessageReceived { sender, msg } => {
                let ConsensusMessage { era_id, payload } = msg;
                self.delegate_to_era(era_id, effect_builder, move |consensus| {
                    consensus.handle_message(sender, payload)
                })
            }
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => {
                let mut effects = effect_builder
                    .announce_proposed_proto_block(proto_block.clone())
                    .ignore();
                effects.extend(
                    self.delegate_to_era(era_id, effect_builder, move |consensus| {
                        consensus.propose(proto_block, block_context)
                    }),
                );
                effects
            }
            Event::ExecutedBlock { .. } => {
                // TODO: Finality signatures
                Effects::new()
            }
            Event::AcceptProtoBlock {
                era_id,
                proto_block,
            } => {
                let mut effects = self.delegate_to_era(era_id, effect_builder, |consensus| {
                    consensus.resolve_validity(&proto_block, true)
                });
                effects.extend(
                    effect_builder
                        .announce_proposed_proto_block(proto_block)
                        .ignore(),
                );
                effects
            }
            Event::InvalidProtoBlock {
                era_id,
                sender: _sender,
                proto_block,
            } => self.delegate_to_era(era_id, effect_builder, |consensus| {
                consensus.resolve_validity(&proto_block, false)
            }),
        }
    }
}
