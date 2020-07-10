use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
    time::Duration,
};

use futures::FutureExt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use tracing::{debug, error, warn};

use crate::{
    components::{
        small_network::NodeId,
        storage::{self, Storage},
        Component,
    },
    effect::{
        requests::{DeployGossiperRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{Deploy, DeployHash},
    utils::{DisplayIter, GossipAction, GossipTable},
    GossipTableConfig,
};

trait ReactorEvent:
    From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
}

impl<T> ReactorEvent for T where
    T: From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
}

/// `DeployGossiper` events.
#[derive(Debug)]
pub enum Event {
    /// A request to begin gossiping a new deploy received from a client.
    Request(DeployGossiperRequest),
    /// The network component gossiped to the included peers.
    GossipedTo {
        deploy_hash: DeployHash,
        peers: HashSet<NodeId>,
    },
    /// The timeout for waiting for a gossip response has elapsed and we should check the response
    /// arrived.
    CheckGossipTimeout {
        deploy_hash: DeployHash,
        peer: NodeId,
    },
    /// The timeout for waiting for the full deploy body has elapsed and we should check the
    /// response arrived.
    CheckGetFromPeerTimeout {
        deploy_hash: DeployHash,
        peer: NodeId,
    },
    /// An incoming gossip network message.
    MessageReceived { sender: NodeId, message: Message },
    /// The result of the `DeployGossiper` putting a deploy to the storage component.  If the
    /// result is `Ok`, the deploy hash should be gossiped onwards.
    PutToStoreResult {
        deploy_hash: DeployHash,
        sender: NodeId,
        result: storage::Result<()>,
    },
    /// The result of the `DeployGossiper` getting a deploy from the storage component.  If the
    /// result is `Ok`, the deploy should be sent to the requesting peer.
    GetFromStoreResult {
        deploy_hash: DeployHash,
        requester: NodeId,
        result: Box<storage::Result<Deploy>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(request) => write!(formatter, "{}", request),
            Event::GossipedTo { deploy_hash, peers } => write!(
                formatter,
                "gossiped {} to {}",
                deploy_hash,
                DisplayIter::new(peers)
            ),
            Event::CheckGossipTimeout { deploy_hash, peer } => write!(
                formatter,
                "check gossip timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::CheckGetFromPeerTimeout { deploy_hash, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::PutToStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "put {} to store", deploy_hash)
                } else {
                    write!(formatter, "failed to put {} to store", deploy_hash)
                }
            }
            Event::GetFromStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "got {} from store", deploy_hash)
                } else {
                    write!(formatter, "failed to get {} from store", deploy_hash)
                }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message {
    /// Gossiped out to random peers to notify them of a `Deploy` we hold.
    Gossip(DeployHash),
    /// Response to a `Gossip` message.  If `is_already_held` is false, the recipient should treat
    /// this as a `GetRequest` and send a `GetResponse` containing the `Deploy`.
    GossipResponse {
        deploy_hash: DeployHash,
        is_already_held: bool,
    },
    /// Sent if a `Deploy` fails to arrive, either after sending a `GossipResponse` with
    /// `is_already_held` set to false, or after a previous `GetRequest`.
    GetRequest(DeployHash),
    /// Sent in response to a `GetRequest`, or to a peer which responded to gossip indicating it
    /// didn't already hold the full `Deploy`.
    GetResponse(Box<Deploy>),
}

impl Display for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Gossip(deploy_hash) => write!(formatter, "gossip({})", deploy_hash),
            Message::GossipResponse {
                deploy_hash,
                is_already_held,
            } => write!(
                formatter,
                "gossip-response({}, {})",
                deploy_hash, is_already_held
            ),
            Message::GetRequest(deploy_hash) => write!(formatter, "get-request({})", deploy_hash),
            Message::GetResponse(deploy) => write!(formatter, "get-response({})", deploy.id()),
        }
    }
}

/// The component which gossips `Deploy`s to peers and handles incoming `Deploy`s which have been
/// gossiped to it.
#[derive(Debug)]
pub(crate) struct DeployGossiper {
    table: GossipTable<DeployHash>,
    gossip_timeout: Duration,
    get_from_peer_timeout: Duration,
}

impl DeployGossiper {
    pub(crate) fn new(config: GossipTableConfig) -> Self {
        DeployGossiper {
            table: GossipTable::new(config),
            gossip_timeout: Duration::from_secs(config.gossip_request_timeout_secs()),
            get_from_peer_timeout: Duration::from_secs(config.get_remainder_timeout_secs()),
        }
    }

    /// Handles a new deploy received from a client by starting to gossip it.
    fn handle_put_from_client<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
    ) -> Effects<Event> {
        if let Some(should_gossip) = self.table.new_complete_data(deploy.id(), None) {
            self.gossip(
                effect_builder,
                *deploy.id(),
                should_gossip.count,
                should_gossip.exclude_peers,
            )
        } else {
            Default::default() // we already completed gossiping this deploy
        }
    }

    /// Gossips the given deploy hash to `count` random peers excluding the indicated ones.
    fn gossip<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        count: usize,
        exclude_peers: HashSet<NodeId>,
    ) -> Effects<Event> {
        let message = Message::Gossip(deploy_hash);
        effect_builder
            .gossip_message(message, count, exclude_peers)
            .event(move |peers| Event::GossipedTo { deploy_hash, peers })
    }

    /// Handles the response from the network component detailing which peers it gossiped to.
    fn gossiped_to<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        peers: HashSet<NodeId>,
    ) -> Effects<Event> {
        // We don't have any peers to gossip to, so pause the process, which will eventually result
        // in the entry being removed.
        if peers.is_empty() {
            self.table.pause(&deploy_hash);
            warn!(
                "paused gossiping {} since no more peers to gossip to",
                deploy_hash
            );
            return Default::default();
        }

        // Set timeouts to check later that the specified peers all responded.
        peers
            .into_iter()
            .map(|peer| {
                effect_builder
                    .set_timeout(self.gossip_timeout)
                    .map(move |_| smallvec![Event::CheckGossipTimeout { deploy_hash, peer }])
                    .boxed()
            })
            .collect()
    }

    /// Checks that the given peer has responded to a previous gossip request we sent it.
    fn check_gossip_timeout<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        peer: NodeId,
    ) -> Effects<Event> {
        match self.table.check_timeout(&deploy_hash, peer) {
            GossipAction::ShouldGossip(should_gossip) => self.gossip(
                effect_builder,
                deploy_hash,
                should_gossip.count,
                should_gossip.exclude_peers,
            ),
            GossipAction::Noop => Default::default(),
            GossipAction::GetRemainder { .. } | GossipAction::AwaitingRemainder => {
                unreachable!("can't have gossiped if we don't hold the complete data")
            }
        }
    }

    /// Checks that the given peer has responded to a previous gossip response or `GetRequest` we
    /// sent it indicating we wanted to get the deploy from it.
    fn check_get_from_peer_timeout<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        peer: NodeId,
    ) -> Effects<Event> {
        match self.table.remove_holder_if_unresponsive(&deploy_hash, peer) {
            GossipAction::ShouldGossip(should_gossip) => self.gossip(
                effect_builder,
                deploy_hash,
                should_gossip.count,
                should_gossip.exclude_peers,
            ),

            GossipAction::GetRemainder { holder } => {
                // The previous peer failed to provide the deploy, so we still need to get it.  Send
                // a `GetRequest` to a different holder and set a timeout to check we got the
                // response.
                let request = Message::GetRequest(deploy_hash);
                let mut effects = effect_builder.send_message(holder, request).ignore();
                effects.extend(
                    effect_builder
                        .set_timeout(self.get_from_peer_timeout)
                        .event(move |_| Event::CheckGetFromPeerTimeout {
                            deploy_hash,
                            peer: holder,
                        }),
                );
                effects
            }

            GossipAction::Noop | GossipAction::AwaitingRemainder => Default::default(),
        }
    }

    /// Handles an incoming gossip request from a peer on the network.
    fn handle_gossip<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        sender: NodeId,
    ) -> Effects<Event> {
        match self.table.new_partial_data(&deploy_hash, sender) {
            GossipAction::ShouldGossip(should_gossip) => {
                // Gossip the deploy hash and send a response to the sender indicating we already
                // hold the deploy.
                let mut effects = self.gossip(
                    effect_builder,
                    deploy_hash,
                    should_gossip.count,
                    should_gossip.exclude_peers,
                );
                let reply = Message::GossipResponse {
                    deploy_hash,
                    is_already_held: true,
                };
                effects.extend(effect_builder.send_message(sender, reply).ignore());
                effects
            }
            GossipAction::GetRemainder { .. } => {
                // Send a response to the sender indicating we want the deploy from them, and set a
                // timeout for this response.
                let reply = Message::GossipResponse {
                    deploy_hash,
                    is_already_held: false,
                };
                let mut effects = effect_builder.send_message(sender, reply).ignore();
                effects.extend(
                    effect_builder
                        .set_timeout(self.get_from_peer_timeout)
                        .event(move |_| Event::CheckGetFromPeerTimeout {
                            deploy_hash,
                            peer: sender,
                        }),
                );
                effects
            }
            GossipAction::Noop | GossipAction::AwaitingRemainder => {
                // Send a response to the sender indicating we already hold the deploy.
                let reply = Message::GossipResponse {
                    deploy_hash,
                    is_already_held: true,
                };
                effect_builder.send_message(sender, reply).ignore()
            }
        }
    }

    /// Handles an incoming gossip response from a peer on the network.
    fn handle_gossip_response<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        is_already_held: bool,
        sender: NodeId,
    ) -> Effects<Event> {
        let mut effects: Effects<_> = Default::default();
        let action = if is_already_held {
            self.table.already_infected(&deploy_hash, sender)
        } else {
            // `sender` doesn't hold the full deploy; treat this as a `GetRequest`.
            effects.extend(self.handle_get_request(effect_builder, deploy_hash, sender));
            self.table.we_infected(&deploy_hash, sender)
        };

        match action {
            GossipAction::ShouldGossip(should_gossip) => effects.extend(self.gossip(
                effect_builder,
                deploy_hash,
                should_gossip.count,
                should_gossip.exclude_peers,
            )),
            GossipAction::Noop => (),
            GossipAction::GetRemainder { .. } | GossipAction::AwaitingRemainder => {
                unreachable!("can't have gossiped if we don't hold the complete data")
            }
        }

        effects
    }

    /// Handles an incoming `GetRequest` from a peer on the network.
    fn handle_get_request<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        sender: NodeId,
    ) -> Effects<Event> {
        // Get the deploy from the storage component then send it to `sender`.
        effect_builder
            .get_deploy_from_storage(deploy_hash)
            .event(move |result| Event::GetFromStoreResult {
                deploy_hash,
                requester: sender,
                result: Box::new(result),
            })
    }

    /// Handles an incoming `GetResponse` from a peer on the network.
    fn handle_get_response<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
        sender: NodeId,
    ) -> Effects<Event> {
        // Put the deploy to the storage component, and potentially start gossiping about it.
        let deploy_hash = *deploy.id();
        effect_builder
            .put_deploy_to_storage(deploy)
            .event(move |result| Event::PutToStoreResult {
                deploy_hash,
                sender,
                result,
            })
    }

    /// Handles the `Ok` case for a `Result` of attempting to put the deploy to the storage
    /// component having received it from the sender.
    fn put_to_store<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        sender: NodeId,
    ) -> Effects<Event> {
        if let Some(should_gossip) = self.table.new_complete_data(&deploy_hash, Some(sender)) {
            self.gossip(
                effect_builder,
                deploy_hash,
                should_gossip.count,
                should_gossip.exclude_peers,
            )
        } else {
            Default::default()
        }
    }

    /// Handles the `Err` case for a `Result` of attempting to put the deploy to the storage
    /// component.
    fn failed_to_put_to_store(
        &mut self,
        deploy_hash: DeployHash,
        storage_error: storage::Error,
    ) -> Effects<Event> {
        self.table.pause(&deploy_hash);
        error!(
            "paused gossiping {} since failed to put to store: {}",
            deploy_hash, storage_error
        );
        Default::default()
    }

    /// Handles the `Ok` case for a `Result` of attempting to get the deploy from the storage
    /// component in order to send it to the requester.
    fn got_from_store<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
        requester: NodeId,
    ) -> Effects<Event> {
        let message = Message::GetResponse(Box::new(deploy));
        effect_builder.send_message(requester, message).ignore()
    }

    /// Handles the `Err` case for a `Result` of attempting to get the deploy from the storage
    /// component.
    fn failed_to_get_from_store(
        &mut self,
        deploy_hash: DeployHash,
        storage_error: storage::Error,
    ) -> Effects<Event> {
        self.table.pause(&deploy_hash);
        error!(
            "paused gossiping {} since failed to get from store: {}",
            deploy_hash, storage_error
        );
        Default::default()
    }
}

impl<REv> Component<REv> for DeployGossiper
where
    REv: From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Request(DeployGossiperRequest::PutFromClient { deploy }) => {
                self.handle_put_from_client(effect_builder, *deploy)
            }
            Event::GossipedTo { deploy_hash, peers } => {
                self.gossiped_to(effect_builder, deploy_hash, peers)
            }
            Event::CheckGossipTimeout { deploy_hash, peer } => {
                self.check_gossip_timeout(effect_builder, deploy_hash, peer)
            }
            Event::CheckGetFromPeerTimeout { deploy_hash, peer } => {
                self.check_get_from_peer_timeout(effect_builder, deploy_hash, peer)
            }
            Event::MessageReceived { message, sender } => match message {
                Message::Gossip(deploy_hash) => {
                    self.handle_gossip(effect_builder, deploy_hash, sender)
                }
                Message::GossipResponse {
                    deploy_hash,
                    is_already_held,
                } => self.handle_gossip_response(
                    effect_builder,
                    deploy_hash,
                    is_already_held,
                    sender,
                ),
                Message::GetRequest(deploy_hash) => {
                    self.handle_get_request(effect_builder, deploy_hash, sender)
                }
                Message::GetResponse(deploy) => {
                    self.handle_get_response(effect_builder, *deploy, sender)
                }
            },
            Event::PutToStoreResult {
                deploy_hash,
                sender,
                result,
            } => match result {
                Ok(()) => self.put_to_store(effect_builder, deploy_hash, sender),
                Err(error) => self.failed_to_put_to_store(deploy_hash, error),
            },
            Event::GetFromStoreResult {
                deploy_hash,
                requester,
                result,
            } => match *result {
                Ok(deploy) => self.got_from_store(effect_builder, deploy, requester),
                Err(error) => self.failed_to_get_from_store(deploy_hash, error),
            },
        }
    }
}
