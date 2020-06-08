//! The consensus component. Provides distributed consensus among the nodes in the network.

use std::fmt;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::{small_network::NodeId, Component},
    effect::{requests::NetworkRequest, Effect, EffectBuilder, Multiple},
};

/// The consensus component.
#[derive(Debug)]
pub(crate) struct Consensus {
    // TODO
}

/// Network message used by the consensus component.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Message {
    /// TODO: create actual message variants
    Dummy,
}

/// Consensus component event.
#[derive(Debug)]
pub(crate) enum Event {
    /// An incoming network message.
    MessageReceived { sender: NodeId, msg: Message },
    // TODO: remove lint relaxation
    #[allow(dead_code)]
    Timer,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Dummy => write!(f, "dummy"),
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::MessageReceived { sender, msg } => write!(f, "msg from {}: {}", sender, msg),
            Event::Timer => write!(f, "timer"),
        }
    }
}

impl<REv> Component<REv> for Consensus
where
    REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        eb: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Timer => todo!(),
            Event::MessageReceived { sender, msg } => self.handle_message(eb, sender, msg),
        }
    }
}

impl Consensus {
    /// Create and initialize a new consensus instance.
    pub(crate) fn new<REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>>(
        _eb: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>) {
        let consensus = Consensus {};

        (consensus, Default::default())
    }

    /// Handles an incoming message
    fn handle_message<REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>>(
        &mut self,
        _eb: EffectBuilder<REv>,
        _sender: NodeId,
        _msg: Message,
    ) -> Multiple<Effect<Event>> {
        Default::default()
    }
}
