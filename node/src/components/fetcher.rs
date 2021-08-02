mod config;
mod event;
mod metrics;
mod tests;

use std::{collections::HashMap, fmt::Debug, time::Duration};

use datasize::DataSize;
use prometheus::Registry;
use smallvec::smallvec;
use tracing::{debug, error, info};

use casper_execution_engine::{shared::newtypes::Blake2bHash, storage::trie::Trie};
use casper_types::{stored_value::StoredValue, Key};

use crate::{
    components::{fetcher::event::FetchResponder, Component},
    effect::{
        requests::{ContractRuntimeRequest, LinearChainRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    types::{Block, BlockByHeight, BlockHash, Deploy, DeployHash, Item, NodeId},
    utils::Source,
    NodeRng,
};

pub use config::Config;
pub use event::{Event, FetchResult};
use metrics::FetcherMetrics;

/// A helper trait constraining `Fetcher` compatible reactor events.
pub trait ReactorEventT<T>:
    From<Event<T>>
    + From<NetworkRequest<NodeId, Message>>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    // Won't be needed when we implement "get block by height" feature in storage.
    + From<LinearChainRequest<NodeId>>
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
        + From<LinearChainRequest<NodeId>>
        + Send
        + 'static,
{
}

pub trait ItemFetcher<T: Item + 'static> {
    fn responders(&mut self) -> &mut HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>;

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
            Some(FetchResult::FromStorage(Box::new(item))),
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
                error!("failed to construct get request: {}", error);
                self.signal(id, None, peer)
            }
        }
    }

    /// Handles signalling responders with the item or `None`.
    fn signal(
        &mut self,
        id: T::Id,
        result: Option<FetchResult<T, NodeId>>,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        let mut effects = Effects::new();
        let mut all_responders = self.responders().remove(&id).unwrap_or_default();
        match result {
            Some(ret) => {
                // signal all responders waiting for this item
                for (_, responders) in all_responders {
                    for responder in responders {
                        effects.extend(responder.respond(Some(ret.clone())).ignore());
                    }
                }
            }
            None => {
                // remove only the peer specific responders for this id
                if let Some(responders) = all_responders.remove(&peer) {
                    for responder in responders {
                        effects.extend(responder.respond(None).ignore());
                    }
                }
                if !all_responders.is_empty() {
                    self.responders().insert(id, all_responders);
                }
            }
        }
        effects
    }
}

/// The component which fetches an item from local storage or asks a peer if it's not in storage.
#[derive(DataSize, Debug)]
pub struct Fetcher<T>
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
            get_from_peer_timeout: Duration::from_secs(config.get_from_peer_timeout()),
            responders: HashMap::new(),
            metrics: FetcherMetrics::new(name, registry)?,
        })
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
    fn get_from_storage<REv: ReactorEventT<Deploy>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: DeployHash,
        peer: NodeId,
    ) -> Effects<Event<Deploy>> {
        effect_builder
            .get_deploys_from_storage(smallvec![id])
            .event(move |mut results| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(results.pop().expect("can only contain one result")),
            })
    }
}

impl ItemFetcher<Block> for Fetcher<Block> {
    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<Block>>>> {
        &mut self.responders
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

impl ItemFetcher<BlockByHeight> for Fetcher<BlockByHeight> {
    fn responders(
        &mut self,
    ) -> &mut HashMap<u64, HashMap<NodeId, Vec<FetchResponder<BlockByHeight>>>> {
        &mut self.responders
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<BlockByHeight>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: u64,
        peer: NodeId,
    ) -> Effects<Event<BlockByHeight>> {
        effect_builder
            .get_block_at_height_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(result.map(Into::into)),
            })
    }
}

type GlobalStorageTrie = Trie<Key, StoredValue>;

impl ItemFetcher<GlobalStorageTrie> for Fetcher<GlobalStorageTrie> {
    fn responders(
        &mut self,
    ) -> &mut HashMap<Blake2bHash, HashMap<NodeId, Vec<FetchResponder<GlobalStorageTrie>>>> {
        &mut self.responders
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv: ReactorEventT<GlobalStorageTrie>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: Blake2bHash,
        peer: NodeId,
    ) -> Effects<Event<GlobalStorageTrie>> {
        effect_builder
            .read_trie(id)
            .event(move |maybe_trie| Event::GetFromStorageResult {
                id,
                peer,
                maybe_item: Box::new(maybe_trie),
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
            Event::Fetch {
                id,
                peer,
                responder,
            } => self.fetch(effect_builder, id, peer, responder),
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
                        self.signal(item.id(), Some(FetchResult::FromPeer(item, peer)), peer)
                    }
                    Source::Client | Source::Ourself => {
                        // TODO - we could possibly also handle this case
                        Effects::new()
                    }
                }
            }
            // We do nothing in the case of having an incoming deploy rejected.
            Event::RejectedRemotely { .. } => Effects::new(),
            Event::AbsentRemotely { id, peer } => {
                info!(%id, %peer, "element absent on the remote node");
                self.signal(id, None, peer)
            }
            Event::TimeoutPeer { id, peer } => {
                info!(%id, %peer, "request timed out");
                self.metrics.timeouts.inc();
                self.signal(id, None, peer)
            }
        }
    }
}
