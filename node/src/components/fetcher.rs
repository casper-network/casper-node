mod event;
mod message;
mod tests;

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    time::Duration,
};

use rand::Rng;
use serde::{de::DeserializeOwned, Serialize};
use smallvec::smallvec;
use tracing::{debug, error, warn};

use crate::{
    components::{fetcher::event::FetchResponder, storage::Storage, Component},
    effect::{
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    small_network::NodeId,
    types::{Deploy, DeployHash},
    GossipConfig,
};

pub use event::{Event, FetchResult, RequestOrigin};
pub use message::Message;

pub trait Item: Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display {
    type Id: Copy + Eq + Hash + Debug + Display + Serialize + DeserializeOwned + Send + Sync;

    fn id(&self) -> &Self::Id;
}

/// A helper trait constraining `Fetcher` compatible reactor events.
pub trait ReactorEventT<T>:
    From<Event<T>>
    + From<NetworkRequest<NodeId, Message<T>>>
    + From<StorageRequest<Storage>>
    + Send
    + 'static
where
    T: Item + 'static,
    <T as Item>::Id: 'static,
{
}

impl<REv, T> ReactorEventT<T> for REv
where
    T: Item + 'static,
    <T as Item>::Id: 'static,
    REv: From<Event<T>>
        + From<NetworkRequest<NodeId, Message<T>>>
        + From<StorageRequest<Storage>>
        + Send
        + 'static,
{
}

pub trait ItemFetcher<T: Item + 'static> {
    fn responders(&mut self) -> &mut HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>;

    fn peer_timeout(&self) -> Duration;

    /// If `maybe_responder` is `Some`, we've been asked to fetch the item by another component of
    /// this node.  We'll try to get it from our own storage component first, and if that fails,
    /// we'll send an outbound request to `peer` for the item.
    ///
    /// If `maybe_responder` is `None`, we're handling an inbound network request from `peer`.
    /// Outside of malicious behavior, we should have the item in our storage component.
    fn fetch<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        maybe_responder: Option<FetchResponder<T>>,
    ) -> Effects<Event<T>> {
        let request_direction = if let Some(responder) = maybe_responder {
            // Capture responder for later signalling.
            let responders = self.responders();
            responders
                .entry(id)
                .or_default()
                .entry(peer)
                .or_default()
                .push(responder);
            RequestOrigin::Internal
        } else {
            RequestOrigin::External
        };

        // Get the item from the storage component.
        self.get_from_store(effect_builder, request_direction, id, peer)
    }

    // Handles attempting to get the item from storage.
    fn get_from_store<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_origin: RequestOrigin,
        id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>>;

    /// Handles the `Ok` case for a `Result` of attempting to get the item from the storage
    /// component in order to send it to the requester.
    fn got_from_store<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_origin: RequestOrigin,
        item: T,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match request_origin {
            RequestOrigin::External => effect_builder
                .send_message(peer, Message::GetResponse(Box::new(item)))
                .ignore(),
            RequestOrigin::Internal => {
                self.signal(*item.id(), Some(FetchResult::FromStore(item)), peer)
            }
        }
    }

    /// Handles the `Err` case for a `Result` of attempting to get the item from the storage
    /// component.
    fn failed_to_get_from_store<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_origin: RequestOrigin,
        id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        if request_origin == RequestOrigin::External {
            warn!("can't provide {} to {}", id, peer);
            Effects::new()
        } else {
            let message = Message::GetRequest(id);
            let mut effects = effect_builder.send_message(peer, message).ignore();

            effects.extend(
                effect_builder
                    .set_timeout(self.peer_timeout())
                    .event(move |_| Event::TimeoutPeer { id, peer }),
            );

            effects
        }
    }

    /// Handles receiving an item from a peer.
    fn got_from_peer<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item: T,
        peer: NodeId,
    ) -> Effects<Event<T>>;

    /// Handles signalling responders with the item or `None`.
    fn signal(
        &mut self,
        id: T::Id,
        result: Option<FetchResult<T>>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        match result {
            Some(ret) => {
                // signal all responders waiting for this item
                if let Some(all_responders) = self.responders().remove(&id) {
                    for (_, responders) in all_responders {
                        for responder in responders {
                            effects.extend(responder.respond(Some(Box::new(ret.clone()))).ignore());
                        }
                    }
                }
            }
            None => {
                // remove only the peer specific responders for this id
                if let Some(responders) = self.responders().entry(id).or_default().remove(&peer) {
                    for responder in responders {
                        effects.extend(responder.respond(None).ignore());
                    }
                }
            }
        }
        effects
    }
}

/// The component which fetches an item from local storage or asks a peer if its not in storage.
#[derive(Debug)]
pub(crate) struct Fetcher<T: Item + 'static> {
    get_from_peer_timeout: Duration,
    responders: HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>,
}

impl<T: Item> Fetcher<T> {
    pub(crate) fn new(config: GossipConfig) -> Self {
        Fetcher {
            get_from_peer_timeout: Duration::from_secs(config.get_remainder_timeout_secs()),
            responders: HashMap::new(),
        }
    }
}

impl ItemFetcher<Deploy> for Fetcher<Deploy> {
    fn responders(
        &mut self,
    ) -> &mut HashMap<DeployHash, HashMap<NodeId, Vec<FetchResponder<Deploy>>>> {
        &mut self.responders
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    /// Gets a `Deploy` from the storage component.
    fn get_from_store<REv: ReactorEventT<Deploy>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request_origin: RequestOrigin,
        id: DeployHash,
        peer: NodeId,
    ) -> Effects<Event<Deploy>> {
        effect_builder
            .get_deploys_from_storage(smallvec![id])
            .event(move |mut results| Event::GetFromStoreResult {
                request_origin,
                id,
                peer,
                result: Box::new(results.pop().expect("can only contain one result")),
            })
    }

    /// Handles receiving a `Deploy` from a peer.
    fn got_from_peer<REv: ReactorEventT<Deploy>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item: Deploy,
        peer: NodeId,
    ) -> Effects<Event<Deploy>> {
        effect_builder
            .put_deploy_to_storage(item.clone())
            .event(move |result| Event::StoredFromPeerResult {
                item: Box::new(item),
                peer,
                result,
            })
    }
}

impl<T: Item + 'static, REv: ReactorEventT<T>> Component<REv> for Fetcher<T>
where
    Fetcher<T>: ItemFetcher<T>,
{
    type Event = Event<T>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Fetch {
                id,
                peer,
                responder,
            } => self.fetch(effect_builder, id, peer, Some(responder)),
            Event::TimeoutPeer { id, peer } => self.signal(id, None, peer),
            Event::MessageReceived {
                payload,
                sender: peer,
            } => match payload {
                Message::GetRequest(id) => self.fetch(effect_builder, id, peer, None),
                Message::GetResponse(item) => self.got_from_peer(effect_builder, *item, peer),
            },
            Event::GetFromStoreResult {
                request_origin,
                id,
                peer,
                result,
            } => match *result {
                Ok(item) => self.got_from_store(effect_builder, request_origin, item, peer),
                Err(_) => self.failed_to_get_from_store(effect_builder, request_origin, id, peer),
            },
            Event::StoredFromPeerResult { item, peer, result } => match result {
                Ok(_) => self.signal(*item.id(), Some(FetchResult::FromPeer(*item, peer)), peer),
                Err(error) => {
                    error!(
                        "received item {} from peer {} but failed to put it to store: {}",
                        *item.id(),
                        peer,
                        error
                    );
                    Effects::new()
                }
            },
        }
    }
}
