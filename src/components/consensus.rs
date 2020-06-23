//! The consensus component. Provides distributed consensus among the nodes in the network.

mod consensus_protocol;
mod consensus_service;
mod traits;
// TODO: remove when we actually use the deploy buffer
#[allow(unused)]
mod deploy_buffer;
// TODO: remove when we actually construct a Pothole era
#[allow(unused)]
mod pothole;
// TODO: remove when we actually construct a Pothole era
#[allow(unused)]
mod protocols;
// TODO: remove when we actually construct a Pothole era
#[allow(unused)]
mod synchronizer;
// TODO: remove when we actually construct a Highway era
#[allow(unused)]
mod highway_core;

use std::fmt::{self, Display, Formatter};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::{small_network::NodeId, Component},
    effect::{requests::NetworkRequest, Effect, EffectBuilder, Multiple},
    types::Block,
};

use consensus_protocol::NodeId as ConsensusNodeId;
use consensus_service::{
    era_supervisor::EraSupervisor,
    traits::{ConsensusService, EraId, Event as ConsensusEvent, MessageWireFormat},
};

/// The consensus component.
#[derive(Debug)]
pub(crate) struct Consensus {
    era_supervisor: EraSupervisor<Block>,
}

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
        }
    }
}

impl<REv> Component<REv> for Consensus
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
                self.handle_message(effect_builder, sender, msg)
            }
        }
    }
}

impl Consensus {
    /// Create and initialize a new consensus instance.
    pub(crate) fn new<REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>>(
        _effect_builder: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>) {
        let consensus = Consensus {
            era_supervisor: EraSupervisor::<Block>::new(),
        };

        (consensus, Default::default())
    }

    fn internal_node_id(&self, _node_id: NodeId) -> ConsensusNodeId {
        todo!("implement some kind of mapping between node ids")
    }

    /// Handles an incoming message
    fn handle_message<REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        sender: NodeId,
        msg: ConsensusMessage,
    ) -> Multiple<Effect<Event>> {
        let msg = MessageWireFormat {
            era_id: msg.era_id,
            sender: self.internal_node_id(sender),
            message_content: msg.payload,
        };

        let _result = self
            .era_supervisor
            .handle_event(ConsensusEvent::IncomingMessage(msg));

        Default::default()
    }
}
