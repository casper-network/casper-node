mod event;
mod message;
mod tests;

use std::{collections::HashMap, time::Duration};

use rand::Rng;
use smallvec::smallvec;
use tracing::{debug, error, warn};

use crate::{
    components::{deploy_fetcher::event::DeployResponder, storage::Storage, Component},
    effect::{
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    small_network::NodeId,
    types::{Deploy, DeployHash},
    GossipTableConfig,
};

pub use event::{Event, RequestDirection};
pub use message::Message;

pub trait ReactorEvent:
    From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
}

impl<T> ReactorEvent for T where
    T: From<Event> + From<NetworkRequest<NodeId, Message>> + From<StorageRequest<Storage>> + Send
{
}

/// The component which fetches a `Deploy` from local storage or asks a peer if its not in storage.
#[derive(Debug)]
pub(crate) struct DeployFetcher {
    get_from_peer_timeout: Duration,
    responders: HashMap<(DeployHash, NodeId), Vec<DeployResponder>>,
}

impl DeployFetcher {
    pub(crate) fn new(config: GossipTableConfig) -> Self {
        DeployFetcher {
            get_from_peer_timeout: Duration::from_secs(config.get_remainder_timeout_secs()),
            responders: HashMap::new(),
        }
    }

    /// If `maybe_responder` is `Some`, we've been asked to fetch the deploy by another component of
    /// this node.  We'll try to get it from our own storage component first, and if that fails,
    /// we'll send an outbound request to `peer` for the deploy.
    ///
    /// If `maybe_responder` is `None`, we're handling an inbound network request from `peer`.
    /// Outside of malicious behavior, we should have the deploy in our storage component.
    fn fetch<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: DeployHash,
        peer: NodeId,
        maybe_responder: Option<DeployResponder>,
    ) -> Effects<Event> {
        let request_direction = if let Some(responder) = maybe_responder {
            self.responders
                .entry((deploy_hash, peer))
                .or_default()
                .push(responder);
            RequestDirection::Outbound
        } else {
            RequestDirection::Inbound
        };

        // Get the deploy from the storage component.
        self.get_from_store(effect_builder, request_direction, deploy_hash, peer)
    }

    /// Gets a `Deploy` from the storage component.
    fn get_from_store<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_direction: RequestDirection,
        deploy_hash: DeployHash,
        peer: NodeId,
    ) -> Effects<Event> {
        effect_builder
            .get_deploys_from_storage(smallvec![deploy_hash])
            .event(move |mut results| Event::GetFromStoreResult {
                request_direction,
                deploy_hash,
                peer,
                result: Box::new(results.pop().expect("can only contain one result")),
            })
    }

    /// Handles the `Ok` case for a `Result` of attempting to get the deploy from the storage
    /// component in order to send it to the requester.
    fn got_from_store<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_direction: RequestDirection,
        deploy: Deploy,
        peer: NodeId,
    ) -> Effects<Event> {
        if request_direction == RequestDirection::Inbound {
            let message = Message::GetResponse(Box::new(deploy));
            effect_builder.send_message(peer, message).ignore()
        } else {
            let mut effects = Effects::new();
            if let Some(responders) = self.responders.remove(&(*deploy.id(), peer)) {
                for responder in responders {
                    effects.extend(
                        responder
                            .respond(Some(Box::new(deploy.to_owned())))
                            .ignore(),
                    );
                }
            }
            effects
        }
    }

    /// Handles the `Err` case for a `Result` of attempting to get the deploy from the storage
    /// component.
    fn failed_to_get_from_store<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_direction: RequestDirection,
        deploy_hash: DeployHash,
        peer: NodeId,
    ) -> Effects<Event> {
        if request_direction == RequestDirection::Inbound {
            warn!("can't provide {} to {}", deploy_hash, peer);
            Effects::new()
        } else {
            let message = Message::GetRequest(deploy_hash);
            let mut effects = effect_builder.send_message(peer, message).ignore();

            effects.extend(
                effect_builder
                    .set_timeout(self.get_from_peer_timeout)
                    .event(move |_| Event::TimeoutPeer { deploy_hash, peer }),
            );

            effects
        }
    }

    /// Handles receiving a deploy from a peer.
    fn got_from_peer<REv: ReactorEvent>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Deploy,
        peer: NodeId,
    ) -> Effects<Event> {
        let deploy_hash = *deploy.id();
        effect_builder
            .put_deploy_to_storage(deploy)
            .event(move |result| Event::PutToStoreResult {
                deploy_hash,
                peer,
                result,
            })
    }

    /// Remove any remaining in flight fetch requests for provided deploy_hash and peer.
    fn timeout_peer(&mut self, deploy_hash: DeployHash, peer: NodeId) -> Effects<Event> {
        let mut effects = Effects::new();
        if let Some(responders) = self.responders.remove(&(deploy_hash, peer)) {
            for responder in responders {
                effects.extend(responder.respond(None).ignore());
            }
        };
        effects
    }
}

impl<REv> Component<REv> for DeployFetcher
where
    REv: ReactorEvent,
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
            Event::FetchDeploy {
                deploy_hash,
                peer,
                responder,
            } => self.fetch(effect_builder, deploy_hash, peer, Some(responder)),
            Event::TimeoutPeer { deploy_hash, peer } => self.timeout_peer(deploy_hash, peer),
            Event::MessageReceived {
                message,
                sender: peer,
            } => match message {
                Message::GetRequest(deploy_hash) => {
                    self.fetch(effect_builder, deploy_hash, peer, None)
                }
                Message::GetResponse(deploy) => self.got_from_peer(effect_builder, *deploy, peer),
            },
            Event::GetFromStoreResult {
                request_direction,
                deploy_hash,
                peer,
                result,
            } => match *result {
                Ok(deploy) => self.got_from_store(effect_builder, request_direction, deploy, peer),
                Err(_) => self.failed_to_get_from_store(
                    effect_builder,
                    request_direction,
                    deploy_hash,
                    peer,
                ),
            },
            Event::PutToStoreResult {
                deploy_hash,
                peer,
                result,
            } => match result {
                Ok(_) => self.get_from_store(
                    effect_builder,
                    RequestDirection::Outbound,
                    deploy_hash,
                    peer,
                ),
                Err(error) => {
                    error!(
                        "received deploy {} from peer {} but failed to put it to store: {}",
                        deploy_hash, peer, error
                    );
                    Effects::new()
                }
            },
        }
    }
}
