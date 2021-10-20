mod config;
mod event;
mod metrics;
mod tests;

use std::{collections::HashMap, fmt::Debug, time::Duration};

use datasize::DataSize;
use prometheus::Registry;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use casper_execution_engine::storage::trie::Trie;
use casper_hashing::Digest;
use casper_types::{Key, StoredValue};

use crate::{
    components::{fetcher::event::FetchResponder, Component},
    effect::{
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockWithMetadata, Deploy,
        DeployHash, Item, NodeId,
    },
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use event::{Event, FetchResult, FetchedData, FetcherError};
use metrics::FetcherMetrics;

/// A helper trait constraining `Fetcher` compatible reactor events.
pub(crate) trait ReactorEventT<T>:
    From<Event<T>>
    + From<NetworkRequest<NodeId, Message>>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
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
        + From<NetworkRequest<NodeId, Message>>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + Send
        + 'static,
{
}

/// Message to be returned by a peer. Indicates if the item could be fetched or not.
#[derive(Serialize, Deserialize)]
pub enum FetchedOrNotFound<T, Id> {
    Fetched(T),
    NotFound(Id),
}

pub(crate) trait ItemFetcher<T: Item + 'static> {
    /// Indicator on whether it is safe to respond to all of our responders. For example, [Deploy]s
    /// and [BlockHeader]s are safe because their [Item::id] is all that is needed for
    /// authentication. But other structures have _finality signatures_ or have substructures that
    /// require validation. These are not infallible, and only the responders corresponding to the
    /// node queried may be responded to.
    const SAFE_TO_RESPOND_TO_ALL: bool;

    fn responders(&mut self) -> &mut HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>;

    fn metrics(&mut self) -> &FetcherMetrics;

    fn peer_timeout(&self) -> Duration;

    /// We've been asked to fetch the item by another component of this node.  We'll try to get it
    /// from our own storage component first, and if that fails, we'll send a request to `peer` for
    /// the item.
    fn fetch<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
        responder: FetchResponder<T>,
    ) -> Effects<Event<T>> {
        // Capture responder for later signalling.
        let responders = self.responders();
        responders
            .entry(id)
            .or_default()
            .entry(peer)
            .or_default()
            .push(responder);

        // Get the item from the storage component.
        self.get_from_storage(effect_builder, id, peer)
    }

    // Handles attempting to get the item from storage.
    fn get_from_storage<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>>;

    /// Handles the `Ok` case for a `Result` of attempting to get the item from the storage
    /// component in order to send it to the requester.
    fn got_from_storage(&mut self, item: T, peer: NodeId) -> Effects<Event<T>> {
        self.signal(
            item.id(),
            Ok(FetchedData::FromStorage {
                item: Box::new(item),
            }),
            peer,
        )
    }

    /// Handles the `Err` case for a `Result` of attempting to get the item from the storage
    /// component.
    fn failed_to_get_from_storage<REv: ReactorEventT<T>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match Message::new_get_request::<T>(&id) {
            Ok(message) => {
                let mut effects = effect_builder.send_message(peer, message).ignore();

                effects.extend(
                    effect_builder
                        .set_timeout(self.peer_timeout())
                        .event(move |_| Event::TimeoutPeer { id, peer }),
                );

                effects
            }
            Err(error) => {
                error!(
                    "failed to construct get request for peer {}: {}",
                    peer, error
                );
                self.signal(
                    id,
                    Err(FetcherError::CouldNotConstructGetRequest { id, peer }),
                    peer,
                )
            }
        }
    }

    /// Sends fetched data to all responders
    fn respond_to_all(
        &mut self,
        id: T::Id,
        fetched_data: FetchedData<T, NodeId>,
    ) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        let all_responders = self.responders().remove(&id).unwrap_or_default();
        for (_peer, responders) in all_responders {
            for responder in responders {
                effects.extend(responder.respond(Ok(fetched_data.clone())).ignore());
            }
        }
        effects
    }

    /// Responds to all responders corresponding to an item/peer with a result. Result could be an
    /// item or a timeout.
    fn send_response_from_peer(
        &mut self,
        id: T::Id,
        result: FetchResult<T, NodeId>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        let mut all_responders = self.responders().remove(&id).unwrap_or_default();
        for responder in all_responders.remove(&peer).into_iter().flatten() {
            effects.extend(responder.respond(result.clone()).ignore());
            if let Err(FetcherError::TimedOut { .. }) = result {
                // Only if there's still a responder waiting for the item we increment the
                // metric. Otherwise we will count every request as timed out, even if the item
                // had been fetched. We increment the metric for every responder as that's how
                // many requests were made in the first place – since requests are duplicated we
                // will request the same item multiple times.
                self.metrics().timeouts.inc();
            }
        }
        if !all_responders.is_empty() {
            self.responders().insert(id, all_responders);
        }
        effects
    }

    /// Handles signalling responders with the item or an error.
    fn signal(
        &mut self,
        id: T::Id,
        result: FetchResult<T, NodeId>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match result {
            Ok(fetched_data_from_storage @ FetchedData::FromStorage { .. }) => {
                debug!(
                    ?fetched_data_from_storage,
                    ?peer,
                    "Got from storage when fetching {:?} from peer",
                    T::TAG,
                );
                self.metrics().found_in_storage.inc();
                // It is always safe to respond to all when we retrieved from storage.
                self.respond_to_all(id, fetched_data_from_storage)
            }
            Ok(fetched_data_from_peer @ FetchedData::FromPeer { .. }) => {
                self.metrics().found_on_peer.inc();
                if Self::SAFE_TO_RESPOND_TO_ALL {
                    self.respond_to_all(id, fetched_data_from_peer)
                } else {
                    self.send_response_from_peer(id, Ok(fetched_data_from_peer), peer)
                }
            }
            Err(_) => self.send_response_from_peer(id, result, peer),
        }
    }
}

/// The component which fetches an item from local storage or asks a peer if it's not in storage.
#[derive(DataSize, Debug)]
pub(crate) struct Fetcher<T>
where
    T: Item + 'static,
{
    get_from_peer_timeout: Duration,
    responders: HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>,
    #[data_size(skip)]
    metrics: FetcherMetrics,
}

impl<T: Item> Fetcher<T> {
    pub(crate) fn new(
        name: &str,
        config: Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetcher {
            get_from_peer_timeout: config.get_from_peer_timeout().into(),
            responders: HashMap::new(),
            metrics: FetcherMetrics::new(name, registry)?,
        })
    }
}

impl ItemFetcher<Deploy> for Fetcher<Deploy> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<DeployHash, HashMap<NodeId, Vec<FetchResponder<Deploy>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    /// Gets a `Deploy` from the storage component.
    fn get_from_storage<REv: ReactorEventT<Deploy>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: DeployHash,
        peer: NodeId,
    ) -> Effects<Event<Deploy>> {
        effect_builder
            .get_deploys_from_storage(vec![id])
            .event(move |mut results| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(results.pop().expect("can only contain one result")),
            })
    }
}

impl ItemFetcher<Block> for Fetcher<Block> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<Block>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<Block>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
        peer: NodeId,
    ) -> Effects<Event<Block>> {
        effect_builder
            .get_block_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(result),
            })
    }
}

impl ItemFetcher<BlockWithMetadata> for Fetcher<BlockWithMetadata> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn responders(
        &mut self,
    ) -> &mut HashMap<u64, HashMap<NodeId, Vec<FetchResponder<BlockWithMetadata>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<BlockWithMetadata>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: u64,
        peer: NodeId,
    ) -> Effects<Event<BlockWithMetadata>> {
        effect_builder
            .get_block_and_sufficient_finality_signatures_by_height_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(result),
            })
    }
}

impl ItemFetcher<BlockHeaderWithMetadata> for Fetcher<BlockHeaderWithMetadata> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn responders(
        &mut self,
    ) -> &mut HashMap<u64, HashMap<NodeId, Vec<FetchResponder<BlockHeaderWithMetadata>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<BlockHeaderWithMetadata>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: u64,
        peer: NodeId,
    ) -> Effects<Event<BlockHeaderWithMetadata>> {
        effect_builder
            .get_block_header_and_sufficient_finality_signatures_by_height_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(result),
            })
    }
}

type GlobalStorageTrie = Trie<Key, StoredValue>;

impl ItemFetcher<GlobalStorageTrie> for Fetcher<GlobalStorageTrie> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<Digest, HashMap<NodeId, Vec<FetchResponder<GlobalStorageTrie>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<GlobalStorageTrie>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: Digest,
        peer: NodeId,
    ) -> Effects<Event<GlobalStorageTrie>> {
        async move {
            let maybe_trie = match effect_builder.get_trie(id).await {
                Ok(maybe_trie) => maybe_trie,
                Err(error) => {
                    error!(?error, "get_trie_request");
                    None
                }
            };
            Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(maybe_trie),
            }
        }
        .event(std::convert::identity)
    }
}

impl ItemFetcher<BlockHeader> for Fetcher<BlockHeader> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<BlockHeader>>>> {
        &mut self.responders
    }

    fn metrics(&mut self) -> &FetcherMetrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<BlockHeader>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
        peer: NodeId,
    ) -> Effects<Event<BlockHeader>> {
        effect_builder
            .get_block_header_from_storage(id)
            .event(move |maybe_block_header| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(maybe_block_header),
            })
    }
}

impl<T, REv> Component<REv> for Fetcher<T>
where
    Fetcher<T>: ItemFetcher<T>,
    T: Item + 'static,
    REv: ReactorEventT<T>,
{
    type Event = Event<T>;
    type ConstructionError = prometheus::Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Fetch(FetcherRequest {
                id,
                peer,
                responder,
            }) => self.fetch(effect_builder, id, peer, responder),
            Event::GetFromStorageResult {
                id,
                peer,
                maybe_item,
            } => match *maybe_item {
                Some(item) => {
                    self.metrics.found_in_storage.inc();
                    self.got_from_storage(item, peer)
                }
                None => self.failed_to_get_from_storage(effect_builder, id, peer),
            },
            Event::GotRemotely { item, source } => {
                match source {
                    Source::Peer(peer) => {
                        self.metrics.found_on_peer.inc();
                        self.signal(item.id(), Ok(FetchedData::FromPeer { item, peer }), peer)
                    }
                    Source::Client | Source::Ourself => {
                        // TODO - we could possibly also handle this case
                        Effects::new()
                    }
                }
            }
            Event::RejectedRemotely { .. } => Effects::new(),
            Event::AbsentRemotely { id, peer } => {
                info!(TAG=%T::TAG, %id, %peer, "item absent on the remote node");
                self.signal(id, Err(FetcherError::Absent { id, peer }), peer)
            }
            Event::TimeoutPeer { id, peer } => {
                info!(TAG=%T::TAG, %id, %peer, "request timed out");
                self.metrics.timeouts.inc();
                self.signal(id, Err(FetcherError::TimedOut { id, peer }), peer)
            }
        }
    }
}
