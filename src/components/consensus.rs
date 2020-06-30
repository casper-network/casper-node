//! The consensus component. Provides distributed consensus among the nodes in the network.

mod consensus_protocol;
mod traits;
// TODO: remove when we actually use the deploy buffer
#[allow(unused)]
mod deploy_buffer;
mod era_supervisor;
// TODO: remove when we actually construct a Pothole era
#[allow(unused)]
mod pothole;
// TODO: remove when we actually construct a Pothole era
#[allow(unused)]
mod protocols;
// TODO: remove when we actually construct a Highway era
#[allow(unused)]
mod highway_core;

#[cfg(test)]
#[allow(unused)]
#[allow(dead_code)]
mod highway_testing;

use std::fmt::{self, Display, Formatter};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::{small_network::NodeId, Component},
    effect::{requests::NetworkRequest, Effect, EffectBuilder, Multiple},
    types::{ExecutedBlock, ProtoBlock},
};

pub(crate) use era_supervisor::{EraId, EraSupervisor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusMessage {
    era_id: EraId,
    payload: Vec<u8>,
}

/// Consensus component event.
#[derive(Debug)]
pub enum Event {
    /// An incoming network message.
    MessageReceived {
        sender: NodeId,
        msg: ConsensusMessage,
    },
    // TODO: remove lint relaxation
    #[allow(dead_code)]
    Timer,
    /// We are receiving the data we require to propose a new block
    NewProtoBlock(ProtoBlock),
    /// We are receiving the information necessary to produce finality signatures
    ExecutedBlock(ExecutedBlock),
    /// The proto-block has been validated and can now be added to the protocol state
    AcceptProtoBlock(ProtoBlock),
    /// The proto-block turned out to be invalid, we might want to accuse/punish/... the sender
    InvalidProtoBlock(NodeId, ProtoBlock),
}

impl Display for ConsensusMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ConsensusMessage {{ era_id: {}, .. }}", self.era_id.0)
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::MessageReceived { sender, msg } => write!(f, "msg from {}: {}", sender, msg),
            Event::Timer => write!(f, "timer"),
            Event::NewProtoBlock(proto_block) => write!(f, "New proto-block: {:?}", proto_block),
            Event::ExecutedBlock(executed_block) => {
                write!(f, "A block has been executed: {:?}", executed_block)
            }
            Event::AcceptProtoBlock(proto_block) => {
                write!(f, "A proto-block has been validated: {:?}", proto_block)
            }
            Event::InvalidProtoBlock(sender, proto_block) => write!(
                f,
                "A proto-block received from {:?} turned out to be invalid: {:?}",
                sender, proto_block
            ),
        }
    }
}

impl<REv> Component<REv> for EraSupervisor
where
    REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Timer => todo!(),
            Event::MessageReceived { sender, msg } => {
                let ConsensusMessage { era_id, payload } = msg;
                self.handle_message(effect_builder, sender, era_id, payload)
            }
            Event::NewProtoBlock(_proto_block) => todo!(),
            Event::ExecutedBlock(_executed_block) => todo!(),
            Event::AcceptProtoBlock(_proto_block) => todo!(),
            Event::InvalidProtoBlock(_sender, _proto_block) => todo!(),
        }
    }
}
