//! This module is home to the libp2p `GossipSub` behavior, used for gossiping this node's listening
//! addresses in order to allow peers to discover and connect to it.

use std::{
    error::Error as StdError,
    io,
    task::{Context, Poll},
};

use libp2p::{
    core::{
        connection::{ConnectionId, ListenerId},
        ConnectedPoint,
    },
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, GossipsubEvent, MessageAuthenticity, Topic,
        ValidationMode,
    },
    swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, ProtocolsHandler},
    Multiaddr, PeerId,
};
use once_cell::sync::Lazy;
use tracing::{trace, warn};

use crate::{components::network::Config, types::NodeId};

static LISTENING_ADDRESSES_TOPIC: Lazy<Topic> =
    Lazy::new(|| Topic::new("listening-addresses".into()));

#[derive(Debug)]
pub(in crate::components::network) struct ListeningAddresses {
    pub source: PeerId,
    pub addresses: Vec<Multiaddr>,
}

/// Implementor of the libp2p `NetworkBehaviour` for gossiping addresses.
pub(in crate::components::network) struct Behavior {
    gossipsub: Gossipsub,
    our_id: NodeId,
}

impl Behavior {
    pub(in crate::components::network) fn new(config: &Config, our_id: NodeId) -> Self {
        let gossipsub_config = GossipsubConfigBuilder::new()
            .heartbeat_interval(config.gossip_heartbeat_interval.into())
            .max_transmit_size(config.gossip_max_message_size as usize)
            .duplicate_cache_time(config.gossip_duplicate_cache_timeout.into())
            .validation_mode(ValidationMode::Permissive)
            .build();
        let our_peer_id = match &our_id {
            NodeId::P2p(peer_id) => peer_id.clone(),
            _ => unreachable!(),
        };
        let mut gossipsub =
            Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config);
        gossipsub.subscribe(LISTENING_ADDRESSES_TOPIC.clone());
        Behavior { gossipsub, our_id }
    }

    /// Gossips our listening addresses.
    pub(in crate::components::network) fn publish_our_addresses(
        &mut self,
        listening_addresses: Vec<Multiaddr>,
    ) {
        let serialized_message = bincode::serialize(&listening_addresses).unwrap_or_else(|error| {
            warn!(
                %error,
                message = ?listening_addresses,
                "{}: failed to serialize",
                self.our_id
            );
            vec![]
        });

        if let Err(error) = self
            .gossipsub
            .publish(&*LISTENING_ADDRESSES_TOPIC, serialized_message)
        {
            warn!(?error, "{}: failed to gossip our addresses", self.our_id);
        } else {
            trace!("{}: gossiped our addresses", self.our_id);
        }
    }

    /// Called when `self.gossipsub` generates an event.
    ///
    /// Returns a peer's listening addresses if the event provided any.
    fn handle_generated_event(&mut self, event: GossipsubEvent) -> Option<ListeningAddresses> {
        match event {
            GossipsubEvent::Message(received_from, _, message) => {
                trace!(?message, "{}: received addresses via gossip", self.our_id);

                let source = match &message.source {
                    Some(peer_id) => peer_id.clone(),
                    None => {
                        warn!(
                            ?message,
                            "{}: received gossiped listening addresses with no source ID",
                            self.our_id
                        );
                        return None;
                    }
                };

                match bincode::deserialize::<Vec<Multiaddr>>(&message.data) {
                    Ok(addresses) => {
                        let listening_addresses = ListeningAddresses { source, addresses };
                        return Some(listening_addresses);
                    }
                    Err(error) => warn!(
                        %received_from,
                        ?message,
                        ?error,
                        "{}: failed to deserialize gossiped message",
                        self.our_id
                    ),
                }
            }
            GossipsubEvent::Subscribed { peer_id, topic } => {
                trace!(%peer_id, %topic, "{}: peer subscribed to gossip topic", self.our_id)
            }
            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                trace!(%peer_id, %topic, "{}: peer unsubscribed from gossip topic", self.our_id)
            }
        }
        None
    }
}

impl NetworkBehaviour for Behavior {
    type ProtocolsHandler = <Gossipsub as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = ListeningAddresses;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.gossipsub.new_handler()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.gossipsub.addresses_of_peer(peer_id)
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        self.gossipsub.inject_connected(peer_id);
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        self.gossipsub.inject_disconnected(peer_id);
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.gossipsub
            .inject_connection_established(peer_id, connection_id, endpoint);
    }

    fn inject_address_change(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.gossipsub
            .inject_address_change(peer_id, connection_id, old, new);
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.gossipsub
            .inject_connection_closed(peer_id, connection_id, endpoint);
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        error: &dyn StdError,
    ) {
        self.gossipsub
            .inject_addr_reach_failure(peer_id, addr, error);
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        self.gossipsub.inject_dial_failure(peer_id);
    }

    fn inject_new_listen_addr(&mut self, addr: &Multiaddr) {
        self.gossipsub.inject_new_listen_addr(addr);
    }

    fn inject_expired_listen_addr(&mut self, addr: &Multiaddr) {
        self.gossipsub.inject_expired_listen_addr(addr);
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.gossipsub.inject_new_external_addr(addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn StdError + 'static)) {
        self.gossipsub.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &io::Error>) {
        self.gossipsub.inject_listener_closed(id, reason);
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        self.gossipsub.inject_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        context: &mut Context,
        poll_params: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Simply pass most action variants though.  We're only interested in the `GeneratedEvent`
        // variant.  These can be all be handled without needing to return `Poll::Ready` until we
        // get an incoming message event.
        loop {
            match self.gossipsub.poll(context, poll_params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(event)) => {
                    if let Some(listening_addresses) = self.handle_generated_event(event) {
                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                            listening_addresses,
                        ));
                    }
                }
                Poll::Ready(NetworkBehaviourAction::DialAddress { address }) => {
                    warn!(%address, "should not dial address via addresses-gossiper behavior");
                    return Poll::Ready(NetworkBehaviourAction::DialAddress { address });
                }
                Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id, condition }) => {
                    warn!(%peer_id, "should not dial peer via addresses-gossiper behavior");
                    return Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id, condition });
                }
                Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    });
                }
                Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address }) => {
                    return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address });
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
