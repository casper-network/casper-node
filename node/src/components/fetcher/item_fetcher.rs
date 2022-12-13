use std::{
    collections::{hash_map::Entry, HashMap},
    time::Duration,
};

use async_trait::async_trait;
use futures::future::BoxFuture;
use tracing::{debug, error, trace};

use super::{Error, Event, FetchResponder, FetchedData, ItemHandle, Metrics};
use crate::{
    components::network::blocklist::BlocklistJustification,
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{ContractRuntimeRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    types::{FetcherItem, Item, NodeId},
};

pub(super) enum StoringState<'a, T> {
    Enqueued(BoxFuture<'a, ()>),
    WontStore(T),
}

#[async_trait]
pub(super) trait ItemFetcher<T: FetcherItem + 'static> {
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
        &self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        validation_metadata: T::ValidationMetadata,
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + From<ContractRuntimeRequest> + Send,
    {
        Self::get_from_storage(effect_builder, id.clone()).event(move |result| {
            Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata,
                maybe_item: Box::new(result),
                responder,
            }
        })
    }

    /// Handles attempting to get the item from storage.
    async fn get_from_storage<REv>(effect_builder: EffectBuilder<REv>, id: T::Id) -> Option<T>
    where
        REv: From<StorageRequest> + From<ContractRuntimeRequest> + Send;

    /// Handles the `Err` case for a `Result` of attempting to get the item from storage.
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
            Entry::Occupied(mut entry) => {
                let handle = entry.get_mut();
                if *handle.validation_metadata() != validation_metadata {
                    let error = Error::ValidationMetadataMismatch {
                        id,
                        peer,
                        current: Box::new(handle.validation_metadata().clone()),
                        new: Box::new(validation_metadata),
                    };
                    error!(%error, "failed to fetch");
                    return responder.respond(Err(error)).ignore();
                }
                handle.push_responder(responder);
            }
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
                error!(%peer, %error, "failed to construct get request");

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
        effect_builder: EffectBuilder<REv>,
        peer: NodeId,
        item: Box<T>,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + From<PeerBehaviorAnnouncement> + Send,
    {
        self.metrics().found_on_peer.inc();

        let validation_metadata = match self
            .item_handles()
            .get(&item.id())
            .and_then(|item_handles| item_handles.get(&peer))
        {
            Some(item_handle) => item_handle.validation_metadata(),
            None => {
                debug!(item_id = %item.id(), tag = ?T::TAG, %peer, "got unexpected item from peer");
                return Effects::new();
            }
        };

        if let Err(err) = item.validate(validation_metadata) {
            debug!(%peer, %err, ?item, "peer sent invalid item");
            effect_builder
                .announce_block_peer_with_justification(
                    peer,
                    BlocklistJustification::SentInvalidItem {
                        tag: T::TAG,
                        error_msg: err.to_string(),
                    },
                )
                .ignore()
        } else {
            match Self::put_to_storage(effect_builder, *item.clone()) {
                StoringState::WontStore(item) => self.signal(item.id(), Ok(item), peer),
                StoringState::Enqueued(store_future) => {
                    store_future.event(move |_| Event::PutToStorage { item, peer })
                }
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
                            // Only if there's still a responder waiting for the item we increment
                            // the metric. Otherwise we will count every request as timed out, even
                            // if the item had been fetched.
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
                | error @ Error::CouldNotConstructGetRequest { .. }
                | error @ Error::ValidationMetadataMismatch { .. },
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

    fn put_to_storage<'a, REv>(
        _effect_builder: EffectBuilder<REv>,
        _item: T,
    ) -> StoringState<'a, T>
    where
        REv: From<StorageRequest> + Send;

    /// Handles signalling responders with the item or an error.
    fn signal(
        &mut self,
        id: T::Id,
        result: Result<T, Error<T>>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match result {
            Ok(fetched_item) if Self::SAFE_TO_RESPOND_TO_ALL => {
                self.respond_to_all(id, FetchedData::from_peer(fetched_item, peer))
            }
            Ok(_) => self.send_response_from_peer(id, result, peer),
            Err(_) => self.send_response_from_peer(id, result, peer),
        }
    }
}
