use std::{
    error::Error as StdError,
    io, iter,
    marker::PhantomData,
    task::{Context, Poll},
};

use libp2p::{
    core::{
        connection::{ConnectionId, ListenerId},
        ConnectedPoint,
    },
    request_response::{
        ProtocolSupport, RequestResponse, RequestResponseConfig, RequestResponseEvent,
        RequestResponseMessage,
    },
    swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, ProtocolsHandler},
    Multiaddr, PeerId,
};
use tracing::{trace, warn};

use super::{Codec, IncomingMessage, Message, OutgoingMessage, ProtocolId};
use crate::{
    components::{
        chainspec_loader::Chainspec,
        network::{Config, PayloadT},
    },
    types::NodeId,
};

/// Implementor of the libp2p `NetworkBehaviour` for one-way messages.
///
/// This is a wrapper round a `RequestResponse` where the response type is defined to be the unit
/// value.
pub(in crate::components::network) struct Behavior<P: PayloadT> {
    libp2p_req_resp: RequestResponse<Codec>,
    our_id: NodeId,
    _phantom: PhantomData<P>,
}

impl<P: PayloadT> Behavior<P> {
    pub(in crate::components::network) fn new(
        config: &Config,
        chainspec: &Chainspec,
        our_id: NodeId,
    ) -> Self {
        let codec = Codec::from(config);
        // TODO - use correct protocol version taking upgrades into account.
        let protocol_id =
            ProtocolId::new(&chainspec.genesis.name, &chainspec.genesis.protocol_version);
        let request_response_config = RequestResponseConfig::from(config);
        let libp2p_req_resp = RequestResponse::new(
            codec,
            iter::once((protocol_id, ProtocolSupport::Full)),
            request_response_config,
        );
        Behavior {
            libp2p_req_resp,
            our_id,
            _phantom: PhantomData,
        }
    }

    /// Sends a one-way message to a peer.
    pub(in crate::components::network) fn send_message(
        &mut self,
        outgoing_message: OutgoingMessage<P>,
    ) {
        let serialized_message =
            bincode::serialize(&outgoing_message.message).unwrap_or_else(|error| {
                warn!(
                    %error,
                    message = ?outgoing_message.message,
                    "{}: failed to serialize",
                    self.our_id
                );
                vec![]
            });

        match &outgoing_message.destination {
            NodeId::P2p(destination) => {
                let request_id = self
                    .libp2p_req_resp
                    .send_request(destination, serialized_message);
                trace!("{}: sent one-way message {}", self.our_id, request_id);
            }
            destination => {
                warn!(%destination, "{}: can't send to small_network node ID via libp2p", self.our_id)
            }
        }
    }

    /// Called when `self.libp2p_req_resp` generates an event.
    ///
    /// The only event type which will cause the return to be `Some` is an incoming request.  All
    /// other variants simply involve generating a log message.
    fn handle_generated_event(
        &mut self,
        event: RequestResponseEvent<Vec<u8>, ()>,
    ) -> Option<IncomingMessage<P>> {
        trace!("{}: {:?}", self.our_id, event);

        match event {
            RequestResponseEvent::Message {
                peer,
                message: RequestResponseMessage::Request { request, .. },
            } => match bincode::deserialize::<Message<P>>(&request) {
                Ok(message) => {
                    trace!(?peer, %message, "{}: message received", self.our_id);
                    let received_message = IncomingMessage {
                        source: NodeId::from(peer),
                        message,
                    };
                    return Some(received_message);
                }
                Err(error) => warn!(
                    ?peer,
                    ?error,
                    "{}: failed to deserialize request",
                    self.our_id
                ),
            },
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => {
                // Note that a response will still be emitted immediately after the request has been
                // sent, since `RequestResponseCodec::read_response` for the one-way Codec does not
                // actually read anything from the given I/O stream.
            }
            RequestResponseEvent::OutboundFailure {
                peer,
                request_id,
                error,
            } => {
                warn!(
                    ?peer,
                    ?request_id,
                    ?error,
                    "{}: outbound failure",
                    self.our_id
                )
            }
            RequestResponseEvent::InboundFailure {
                peer,
                request_id,
                error,
            } => {
                warn!(
                    ?peer,
                    ?request_id,
                    ?error,
                    "{}: inbound failure",
                    self.our_id
                )
            }
        }

        None
    }
}

impl<P: PayloadT> NetworkBehaviour for Behavior<P> {
    type ProtocolsHandler = <RequestResponse<Codec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = IncomingMessage<P>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.libp2p_req_resp.new_handler()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.libp2p_req_resp.addresses_of_peer(peer_id)
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        self.libp2p_req_resp.inject_connected(peer_id);
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        self.libp2p_req_resp.inject_disconnected(peer_id);
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.libp2p_req_resp
            .inject_connection_established(peer_id, connection_id, endpoint);
    }

    fn inject_address_change(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.libp2p_req_resp
            .inject_address_change(peer_id, connection_id, old, new);
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.libp2p_req_resp
            .inject_connection_closed(peer_id, connection_id, endpoint);
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        error: &dyn StdError,
    ) {
        self.libp2p_req_resp
            .inject_addr_reach_failure(peer_id, addr, error);
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        self.libp2p_req_resp.inject_dial_failure(peer_id);
    }

    fn inject_new_listen_addr(&mut self, addr: &Multiaddr) {
        self.libp2p_req_resp.inject_new_listen_addr(addr);
    }

    fn inject_expired_listen_addr(&mut self, addr: &Multiaddr) {
        self.libp2p_req_resp.inject_expired_listen_addr(addr);
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.libp2p_req_resp.inject_new_external_addr(addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn StdError + 'static)) {
        self.libp2p_req_resp.inject_listener_error(id, err);
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &io::Error>) {
        self.libp2p_req_resp.inject_listener_closed(id, reason);
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        self.libp2p_req_resp
            .inject_event(peer_id, connection_id, event);
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
            match self.libp2p_req_resp.poll(context, poll_params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(event)) => {
                    if let Some(incoming_message) = self.handle_generated_event(event) {
                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                            incoming_message,
                        ));
                    }
                }
                Poll::Ready(NetworkBehaviourAction::DialAddress { address }) => {
                    warn!(%address, "should not dial address via one-way message behavior");
                    return Poll::Ready(NetworkBehaviourAction::DialAddress { address });
                }
                Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id, condition }) => {
                    warn!(%peer_id, "should not dial peer via one-way message behavior");
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
