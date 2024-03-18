mod config;
mod error;
mod event;
mod fetch_item;
mod fetch_response;
mod fetched_data;
mod fetcher_impls;
mod item_fetcher;
mod item_handle;
mod metrics;
mod tag;
mod tests;

use std::{collections::HashMap, fmt::Debug, time::Duration};

use datasize::DataSize;
use prometheus::Registry;
use tracing::trace;

use crate::{
    components::Component,
    effect::{
        announcements::{
            FetchedNewBlockAnnouncement, FetchedNewFinalitySignatureAnnouncement,
            PeerBehaviorAnnouncement,
        },
        requests::{
            BlockAccumulatorRequest, ContractRuntimeRequest, FetcherRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    protocol::Message,
    types::NodeId,
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::Event;
pub(crate) use fetch_item::{EmptyValidationMetadata, FetchItem};
pub(crate) use fetch_response::FetchResponse;
pub(crate) use fetched_data::FetchedData;
use item_fetcher::{ItemFetcher, StoringState};
use item_handle::ItemHandle;
use metrics::Metrics;
pub(crate) use tag::Tag;

pub(crate) type FetchResult<T> = Result<FetchedData<T>, Error<T>>;
pub(crate) type FetchResponder<T> = Responder<FetchResult<T>>;

/// The component which fetches an item from local component(s) or asks a peer if it's not
/// available locally.
#[derive(DataSize, Debug)]
pub(crate) struct Fetcher<T>
where
    T: FetchItem,
{
    get_from_peer_timeout: Duration,
    item_handles: HashMap<T::Id, HashMap<NodeId, ItemHandle<T>>>,
    #[data_size(skip)]
    name: &'static str,
    #[data_size(skip)]
    metrics: Metrics,
}

impl<T: FetchItem> Fetcher<T> {
    pub(crate) fn new(
        name: &'static str,
        config: &Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetcher {
            get_from_peer_timeout: config.get_from_peer_timeout().into(),
            item_handles: HashMap::new(),
            name,
            metrics: Metrics::new(name, registry)?,
        })
    }
}

impl<T, REv> Component<REv> for Fetcher<T>
where
    Fetcher<T>: ItemFetcher<T>,
    T: FetchItem + 'static,
    REv: From<StorageRequest>
        + From<BlockAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + From<NetworkRequest<Message>>
        + From<PeerBehaviorAnnouncement>
        + From<FetchedNewBlockAnnouncement>
        + From<FetchedNewFinalitySignatureAnnouncement>
        + Send,
{
    type Event = Event<T>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!(?event, "Fetcher: handling event");
        match event {
            Event::Fetch(FetcherRequest {
                id,
                peer,
                validation_metadata,
                responder,
            }) => self.fetch(effect_builder, id, peer, validation_metadata, responder),
            Event::GetLocallyResult {
                id,
                peer,
                validation_metadata,
                maybe_item,
                responder,
            } => match maybe_item {
                Some(item) => {
                    self.metrics().found_in_storage.inc();
                    responder
                        .respond(Ok(FetchedData::from_storage(item)))
                        .ignore()
                }
                None => self.failed_to_get_locally(
                    effect_builder,
                    id,
                    peer,
                    validation_metadata,
                    responder,
                ),
            },
            Event::GotRemotely { item, source } => match source {
                Source::PeerGossiped(peer) | Source::Peer(peer) => {
                    self.got_from_peer(effect_builder, peer, item)
                }
                Source::Client | Source::SpeculativeExec | Source::Ourself => Effects::new(),
            },
            Event::GotInvalidRemotely { .. } => Effects::new(),
            Event::AbsentRemotely { id, peer } => {
                trace!(TAG=%T::TAG, %id, %peer, "item absent on the remote node");
                self.signal(
                    id.clone(),
                    Err(Error::Absent {
                        id: Box::new(id),
                        peer,
                    }),
                    peer,
                )
            }
            Event::RejectedRemotely { id, peer } => {
                trace!(TAG=%T::TAG, %id, %peer, "peer rejected fetch request");
                self.signal(
                    id.clone(),
                    Err(Error::Rejected {
                        id: Box::new(id),
                        peer,
                    }),
                    peer,
                )
            }
            Event::TimeoutPeer { id, peer } => self.signal(
                id.clone(),
                Err(Error::TimedOut {
                    id: Box::new(id),
                    peer,
                }),
                peer,
            ),
            Event::PutToStorage { item, peer } => {
                let mut effects =
                    Self::announce_fetched_new_item(effect_builder, (*item).clone(), peer).ignore();
                effects.extend(self.signal(item.fetch_id(), Ok(*item), peer));
                effects
            }
        }
    }

    fn name(&self) -> &str {
        self.name
    }
}
