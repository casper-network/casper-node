mod event;
mod message;
mod tests;

use std::{collections::HashSet, time::Duration};

use futures::FutureExt;
use rand::Rng;
use smallvec::smallvec;
use tracing::{debug, error};

use crate::{
    components::{
        small_network::NodeId,
        storage::{self, Storage},
        Component,
    },
    effect::{
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{Deploy, DeployHash},
    utils::{GossipAction, GossipTable},
    GossipTableConfig,
};

pub use event::Event;
pub use message::Message;

trait ReactorEvent:
    From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
}

impl<T> ReactorEvent for T where
    T: From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
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

    /// Handles a new deploy received from somewhere other than a peer (e.g. the HTTP API server).
    fn handle_deploy_received<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
    ) -> Effects<Event> {
        // Put the deploy to the storage component.
        let deploy_hash = *deploy.id();
        effect_builder
            .put_deploy_to_storage(deploy)
            .event(move |result| Event::PutToStoreResult {
                deploy_hash,
                maybe_sender: None,
                result,
            })
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
            debug!(
                "paused gossiping {} since no more peers to gossip to",
                deploy_hash
            );
            return Effects::new();
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
            GossipAction::Noop => Effects::new(),
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

            GossipAction::Noop | GossipAction::AwaitingRemainder => Effects::new(),
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
        let mut effects: Effects<_> = Effects::new();
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
            .get_deploys_from_storage(smallvec![deploy_hash])
            .event(move |mut results| Event::GetFromStoreResult {
                deploy_hash,
                requester: sender,
                result: Box::new(results.pop().expect("can only contain one result")),
            })
    }

    /// Handles an incoming `GetResponse` from a peer on the network.
    fn handle_get_response<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
        sender: NodeId,
    ) -> Effects<Event> {
        // Put the deploy to the storage component.
        let deploy_hash = *deploy.id();
        effect_builder
            .put_deploy_to_storage(deploy)
            .event(move |result| Event::PutToStoreResult {
                deploy_hash,
                maybe_sender: Some(sender),
                result,
            })
    }

    /// Handles the `Ok` case for a `Result` of attempting to put the deploy to the storage
    /// component having received it from the sender (for the `Some` case) or from our own HTTP API
    /// server (the `None` case).
    fn handle_put_to_store_success<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        maybe_sender: Option<NodeId>,
    ) -> Effects<Event> {
        if let Some(should_gossip) = self.table.new_complete_data(&deploy_hash, maybe_sender) {
            self.gossip(
                effect_builder,
                deploy_hash,
                should_gossip.count,
                should_gossip.exclude_peers,
            )
        } else {
            Effects::new()
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
        Effects::new()
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
        Effects::new()
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
            Event::DeployReceived { deploy } => {
                self.handle_deploy_received(effect_builder, *deploy)
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
                maybe_sender,
                result,
            } => match result {
                Ok(_) => {
                    self.handle_put_to_store_success(effect_builder, deploy_hash, maybe_sender)
                }
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
