use std::{collections::HashMap, time::Duration};

use tracing::{error, trace, warn};

use super::{Error, Event, FetchResponder, FetchedData, Metrics};
use crate::{
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{ContractRuntimeRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    types::{FetcherItem, Item, NodeId},
};

pub(crate) trait ItemFetcher<T: FetcherItem + 'static> {
    /// Indicator on whether it is safe to respond to all of our responders. For example, [Deploy]s
    /// and [BlockHeader]s are safe because their [Item::id] is all that is needed for
    /// authentication. But other structures have _finality signatures_ or have substructures that
    /// require validation. These are not infallible, and only the responders corresponding to the
    /// node queried may be responded to.
    const SAFE_TO_RESPOND_TO_ALL: bool;

    fn responders(&mut self) -> &mut HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>;

    fn validation_metadata(&self) -> &T::ValidationMetadata;

    fn metrics(&mut self) -> &Metrics;

    fn peer_timeout(&self) -> Duration;

    /// We've been asked to fetch the item by another component of this node.  We'll try to get it
    /// from our own storage component first, and if that fails, we'll send a request to `peer` for
    /// the item.
    fn fetch<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        validation_metadata: T::ValidationMetadata,
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + From<ContractRuntimeRequest> + Send,
    {
        // Get the item from the storage component.
        self.get_from_storage(effect_builder, id, peer, validation_metadata, responder)
    }

    // Handles attempting to get the item from storage.
    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        validation_metadata: T::ValidationMetadata,
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + From<ContractRuntimeRequest> + Send;

    /// Handles the `Err` case for a `Result` of attempting to get the item from the storage
    /// component.
    fn failed_to_get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        _validation_metadata: T::ValidationMetadata, // TODO
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>>
    where
        <T as Item>::Id: 'static,
        REv: From<NetworkRequest<Message>> + Send,
    {
        let peer_timeout = self.peer_timeout();
        // Capture responder for later signalling.
        let responders = self.responders();
        responders
            .entry(id.clone())
            .or_default()
            .entry(peer)
            .or_default()
            .push(responder);
        match Message::new_get_request::<T>(&id) {
            Ok(message) => {
                self.metrics().fetch_total.inc();
                async move {
                    effect_builder.send_message(peer, message).await;

                    effect_builder.set_timeout(peer_timeout).await
                }
            }
            .event(move |_| Event::TimeoutPeer { id, peer }),
            Err(error) => {
                error!(
                    "failed to construct get request for peer {}: {}",
                    peer, error
                );
                self.signal(
                    id.clone(),
                    Err(Error::CouldNotConstructGetRequest { id, peer }),
                    peer,
                )
            }
        }
    }

    fn got_from_peer<REv>(
        &mut self,
        peer: NodeId,
        item: Box<T>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + From<PeerBehaviorAnnouncement> + Send,
    {
        self.metrics().found_on_peer.inc();

        if let Err(err) = item.validate(self.validation_metadata()) {
            warn!(?peer, ?err, ?item, "peer sent invalid item, banning peer");
            effect_builder.announce_disconnect_from_peer(peer).ignore()
        } else {
            match self.put_to_storage(*item.clone(), peer, effect_builder) {
                None => self.signal(item.id(), Ok(*item), peer),
                Some(effects) => effects,
            }
        }
    }

    /// Sends fetched data to all responders
    fn respond_to_all(&mut self, id: T::Id, fetched_data: FetchedData<T>) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        let all_responders = self.responders().remove(&id).unwrap_or_default();
        for (_peer, responders) in all_responders {
            for responder in responders {
                effects.extend(responder.respond(Ok(fetched_data.clone())).ignore());
            }
        }
        effects
    }

    /// Responds to all responders corresponding to a specific item-peer combination with a result.
    fn send_response_from_peer(
        &mut self,
        id: T::Id,
        result: Result<T, Error<T>>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        let mut all_responders = self.responders().remove(&id).unwrap_or_default();
        match result {
            Ok(item) => {
                // Since this is a success, we can safely respond to all awaiting processes.
                for responder in all_responders.remove(&peer).into_iter().flatten() {
                    effects.extend(
                        responder
                            .respond(Ok(FetchedData::from_peer(item.clone(), peer)))
                            .ignore(),
                    );
                }
            }
            Err(error @ Error::TimedOut { .. }) => {
                let mut responders = all_responders.remove(&peer).into_iter().flatten();
                // We take just one responder as only one request had timed out. We want to avoid
                // prematurely failing too many waiting processes since other requests may still
                // succeed before timing out.
                if let Some(responder) = responders.next() {
                    effects.extend(responder.respond(Err(error)).ignore());
                    // Only if there's still a responder waiting for the item we increment the
                    // metric. Otherwise we will count every request as timed out, even if the item
                    // had been fetched.
                    trace!(TAG=%T::TAG, %id, %peer, "request timed out");
                    self.metrics().timeouts.inc();
                }

                let responders: Vec<_> = responders.collect();
                if !responders.is_empty() {
                    all_responders.insert(peer, responders);
                }
            }
            Err(
                error @ Error::Absent { .. }
                | error @ Error::Rejected { .. }
                | error @ Error::CouldNotConstructGetRequest { .. },
            ) => {
                // For all other error variants we can safely respond with failure as there's no
                // chance for the request to succeed.
                for responder in all_responders.remove(&peer).into_iter().flatten() {
                    effects.extend(responder.respond(Err(error.clone())).ignore());
                }
            }
        }
        if !all_responders.is_empty() {
            self.responders().insert(id, all_responders);
        }
        effects
    }

    fn put_to_storage<REv>(
        &self,
        _item: T,
        _peer: NodeId,
        _effect_builder: EffectBuilder<REv>,
    ) -> Option<Effects<Event<T>>>
    where
        REv: From<StorageRequest> + Send,
    {
        todo!()
    }

    /// Handles signalling responders with the item or an error.
    fn signal(&mut self, id: T::Id, result: Result<T, Error<T>>, peer: NodeId) -> Effects<Event<T>>
    where
        T: 'static,
    {
        match result {
            Ok(fetched_item) if Self::SAFE_TO_RESPOND_TO_ALL => {
                self.respond_to_all(id, FetchedData::from_peer(fetched_item, peer))
            }
            Ok(_) => self.send_response_from_peer(id, result, peer),
            Err(_) => self.send_response_from_peer(id, result, peer),
        }
    }
}
