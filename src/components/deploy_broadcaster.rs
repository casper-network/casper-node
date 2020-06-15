use std::fmt::{self, Display, Formatter};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    components::{small_network::NodeId, storage::Storage, Component},
    effect::{
        requests::{DeployBroadcasterRequest, NetworkRequest, StorageRequest},
        Effect, EffectBuilder, EffectExt, Multiple,
    },
    types::Deploy,
};

/// Deploy broadcaster events.
#[derive(Debug)]
pub(crate) enum Event {
    Request(DeployBroadcasterRequest),
    /// An incoming broadcast network message.
    MessageReceived {
        sender: NodeId,
        message: Message,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(request) => write!(formatter, "{}", request),
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Message {
    /// A new `Deploy` received from a peer via the small network component.  Since this has been
    /// received via another node's broadcast, this receiving node should not broadcast it.
    Put(Box<Deploy>),
}

impl Display for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Put(deploy) => write!(formatter, "put({})", deploy.id()),
        }
    }
}

/// The component which broadcasts `Deploy`s to peers and handles incoming `Deploy`s which have been
/// broadcast to it.
#[derive(Default)]
pub(crate) struct DeployBroadcaster {}

impl DeployBroadcaster {
    pub(crate) fn new() -> Self {
        DeployBroadcaster {}
    }
}

impl<REv> Component<REv> for DeployBroadcaster
where
    REv: From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Request(DeployBroadcasterRequest::PutFromClient { deploy }) => {
                // Incoming HTTP request - broadcast the `Deploy`.
                effect_builder
                    .broadcast_message(Message::Put(deploy))
                    .ignore()
            }
            Event::MessageReceived {
                message: Message::Put(deploy),
                sender: _,
            } => {
                // Incoming broadcast message - just store the `Deploy`.
                effect_builder.put_deploy(*deploy).ignore()
            }
        }
    }
}
