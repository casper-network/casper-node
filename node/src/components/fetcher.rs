mod config;
mod error;
mod event;
mod fetch_response;
mod fetched_data;
mod fetcher_impls;
mod item_fetcher;
mod metrics;
mod tests;

use std::{collections::HashMap, fmt::Debug, time::Duration};

use datasize::DataSize;
use num_rational::Ratio;
use prometheus::Registry;
use tracing::{debug, error, info, trace, warn};

use crate::{
    components::{
        linear_chain::{self, BlockSignatureError},
        Component,
    },
    effect::{
        announcements::BlocklistAnnouncement,
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    protocol::Message,
    types::{BlockHeader, BlockSignatures, FetcherItem, NodeId},
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::Event;
pub(crate) use fetch_response::FetchResponse;
pub(crate) use fetched_data::FetchedData;
pub(crate) use item_fetcher::ItemFetcher;
use metrics::Metrics;

pub(crate) type FetchResult<T> = Result<FetchedData<T>, Error<T>>;
pub(crate) type FetchResponder<T> = Responder<FetchResult<T>>;

/// The component which fetches an item from local storage or asks a peer if it's not in storage.
#[derive(DataSize, Debug)]
pub(crate) struct Fetcher<T>
where
    T: FetcherItem,
{
    get_from_peer_timeout: Duration,
    responders: HashMap<T::Id, HashMap<NodeId, Vec<FetchResponder<T>>>>,
    #[data_size(skip)]
    metrics: Metrics,
    #[data_size(skip)]
    validation_metadata: T::ValidationMetadata,
}

impl<T: FetcherItem> Fetcher<T>
where
    T::ValidationMetadata: Default,
{
    pub(crate) fn new(
        name: &str,
        config: &Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetcher {
            get_from_peer_timeout: config.get_from_peer_timeout().into(),
            responders: HashMap::new(),
            metrics: Metrics::new(name, registry)?,
            validation_metadata: Default::default(),
        })
    }
}

impl<T: FetcherItem> Fetcher<T> {
    pub(crate) fn new_with_metadata(
        name: &str,
        config: &Config,
        registry: &Registry,
        validation_metadata: T::ValidationMetadata,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetcher {
            get_from_peer_timeout: config.get_from_peer_timeout().into(),
            responders: HashMap::new(),
            metrics: Metrics::new(name, registry)?,
            validation_metadata,
        })
    }
}

impl<T, REv> Component<REv> for Fetcher<T>
where
    Fetcher<T>: ItemFetcher<T>,
    T: FetcherItem + 'static,
    REv: From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<NetworkRequest<Message>>
        + From<BlocklistAnnouncement>
        + Send,
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
                validation_metadata,
                responder,
            }) => self.fetch(effect_builder, id, peer, validation_metadata, responder),
            Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata,
                maybe_item,
                responder,
            } => match *maybe_item {
                Some(item) => {
                    self.metrics().found_in_storage.inc();
                    responder
                        .respond(Ok(FetchedData::from_storage(item)))
                        .ignore()
                }
                None => self.failed_to_get_from_storage(
                    effect_builder,
                    id,
                    peer,
                    validation_metadata,
                    responder,
                ),
            },
            Event::GotRemotely { item, source } => match source {
                Source::Peer(peer) => self.got_from_peer(peer, item, effect_builder),
                Source::Client | Source::Ourself => Effects::new(),
            },
            Event::GotInvalidRemotely { .. } => Effects::new(),
            Event::AbsentRemotely { id, peer } => {
                trace!(TAG=%T::TAG, %id, %peer, "item absent on the remote node");
                self.signal(id.clone(), Err(Error::Absent { id, peer }), peer)
            }
            Event::RejectedRemotely { id, peer } => {
                trace!(TAG=%T::TAG, %id, %peer, "peer rejected fetch request");
                self.signal(id.clone(), Err(Error::Rejected { id, peer }), peer)
            }
            Event::TimeoutPeer { id, peer } => {
                self.signal(id.clone(), Err(Error::TimedOut { id, peer }), peer)
            }
            Event::PutToStorage { item, peer } => self.signal(item.id(), Ok(*item), peer),
        }
    }
}

/// Returns `true` if the cumulative weight of the given signatures is sufficient for the given
/// block using the specified `fault_tolerance_fraction`.
///
/// Note that signatures are _not_ cryptographically verified in this function.
async fn has_enough_block_signatures<REv>(
    effect_builder: EffectBuilder<REv>,
    block_header: &BlockHeader,
    block_signatures: &BlockSignatures,
    fault_tolerance_fraction: Ratio<u64>,
) -> bool
where
    REv: From<StorageRequest> + From<ContractRuntimeRequest> + From<BlocklistAnnouncement> + Send,
{
    let validator_weights =
        match linear_chain::era_validator_weights_for_block(block_header, effect_builder).await {
            Ok((_, validator_weights)) => validator_weights,
            Err(error) => {
                warn!(
                    ?error,
                    ?block_header,
                    ?block_signatures,
                    "failed to get validator weights for given block"
                );
                return false;
            }
        };

    match linear_chain::check_sufficient_block_signatures(
        &validator_weights,
        fault_tolerance_fraction,
        Some(block_signatures),
    ) {
        Err(error @ BlockSignatureError::InsufficientWeightForFinality { .. }) => {
            info!(?error, "insufficient block signatures from storage");
            false
        }
        Err(error @ BlockSignatureError::BogusValidators { .. }) => {
            error!(?error, "bogus validators block signature from storage");
            false
        }
        Ok(_) => true,
    }
}
