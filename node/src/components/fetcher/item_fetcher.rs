use std::{
    collections::{hash_map::Entry, HashMap},
    time::Duration,
};

use tracing::{error, trace, warn};

use super::{Error, Event, FetchResponder, FetchedData, ItemHandle, Metrics};
use crate::{
    effect::{
        announcements::BlocklistAnnouncement,
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

    fn item_handles(&mut self) -> &mut HashMap<T::Id, HashMap<NodeId, ItemHandle<T>>>;

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
        validation_metadata: T::ValidationMetadata,
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>>
    where
        <T as Item>::Id: 'static,
        REv: From<NetworkRequest<Message>> + Send,
    {
        let peer_timeout = self.peer_timeout();
        // Capture responder for later signalling.
        let item_handles = self.item_handles();
        match item_handles.entry(id.clone()).or_default().entry(peer) {
            Entry::Occupied(mut entry) => entry.get_mut().push_responder(responder),
            Entry::Vacant(entry) => {
                entry.insert(ItemHandle::new(validation_metadata, responder));
            }
        }
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
        REv: From<StorageRequest> + From<BlocklistAnnouncement> + Send,
    {
        self.metrics().found_on_peer.inc();

        let validation_metadata = match self
            .item_handles()
            .get(&item.id())
            .and_then(|item_handles| item_handles.get(&peer))
        {
            Some(item_handle) => item_handle.validation_metadata(),
            None => return Effects::new(),
        };

        if let Err(err) = item.validate(&validation_metadata) {
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
        let item_handles = self.item_handles().remove(&id).unwrap_or_default();
        for (_peer, item_handle) in item_handles {
            for responder in item_handle.take_responders() {
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
        let mut item_handles = self.item_handles().remove(&id).unwrap_or_default();
        match result {
            Ok(item) => {
                // Since this is a success, we can safely respond to all awaiting processes.
                for responder in item_handles
                    .remove(&peer)
                    .map(ItemHandle::take_responders)
                    .unwrap_or_default()
                {
                    effects.extend(
                        responder
                            .respond(Ok(FetchedData::from_peer(item.clone(), peer)))
                            .ignore(),
                    );
                }
            }
            Err(error @ Error::TimedOut { .. }) => {
                // We take just one responder as only one request had timed out. We want to avoid
                // prematurely failing too many waiting processes since other requests may still
                // succeed before timing out.
                let should_remove_item_handle = match item_handles.get_mut(&peer) {
                    Some(item_handle) => {
                        if let Some(responder) = item_handle.pop_front_responder() {
                            effects.extend(responder.respond(Err(error)).ignore());
                            // Only if there's still a responder waiting for the item we increment the
                            // metric. Otherwise we will count every request as timed out, even if the item
                            // had been fetched.
                            trace!(TAG=%T::TAG, %id, %peer, "request timed out");
                            self.metrics().timeouts.inc();
                        }
                        item_handle.has_no_responders()
                    }
                    None => false,
                };
                if should_remove_item_handle {
                    item_handles.remove(&peer);
                }
            }
            Err(
                error @ Error::Absent { .. }
                | error @ Error::Rejected { .. }
                | error @ Error::CouldNotConstructGetRequest { .. },
            ) => {
                // For all other error variants we can safely respond with failure as there's no
                // chance for the request to succeed.
                for responder in item_handles
                    .remove(&peer)
                    .map(ItemHandle::take_responders)
                    .unwrap_or_default()
                {
                    effects.extend(responder.respond(Err(error.clone())).ignore());
                }
            }
        }
        if !item_handles.is_empty() {
            self.item_handles().insert(id, item_handles);
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
