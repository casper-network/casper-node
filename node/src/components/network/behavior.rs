use derive_more::From;
use libp2p::{
    ping::{Ping, PingConfig, PingEvent},
    NetworkBehaviour,
};

use super::{
    Config, OneWayIncomingMessage, OneWayMessageBehavior, OneWayOutgoingMessage, PayloadT,
};
use crate::{components::chainspec_loader::Chainspec, types::NodeId};

/// An enum defining the top-level events passed to the swarm's handler.  This will be received in
/// the swarm's handler wrapped in a `SwarmEvent::Behaviour`.
#[derive(Debug, From)]
pub(super) enum SwarmBehaviorEvent<P: PayloadT> {
    #[from]
    OneWayMessage(OneWayIncomingMessage<P>),
    #[from]
    Ping(PingEvent),
}

/// The top-level behavior used in the libp2p swarm.  It holds all subordinate behaviors required to
/// operate the network component.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "SwarmBehaviorEvent<P>", event_process = false)]
pub(super) struct Behavior<P: PayloadT> {
    one_way_message_behavior: OneWayMessageBehavior<P>,
    ping: Ping,
}

impl<P: PayloadT> Behavior<P> {
    pub(super) fn new(config: &Config, chainspec: &Chainspec, our_id: NodeId) -> Self {
        let one_way_message_behavior = OneWayMessageBehavior::new(&config, chainspec, our_id);
        let ping = Ping::new(PingConfig::new().with_keep_alive(true));
        Behavior {
            one_way_message_behavior,
            ping,
        }
    }

    pub(super) fn send_one_way_message(&mut self, outgoing_message: OneWayOutgoingMessage<P>) {
        self.one_way_message_behavior.send_message(outgoing_message);
    }
}
