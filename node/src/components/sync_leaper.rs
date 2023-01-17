//! The Sync Leaper
mod error;
mod event;
mod leap_activity;
mod leap_state;
mod metrics;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Instant};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{error, info, warn};

use crate::{
    components::{
        fetcher::{self, FetchResult, FetchedData},
        Component,
    },
    effect::{requests::FetcherRequest, EffectBuilder, EffectExt, Effects},
    types::{Chainspec, NodeId, SyncLeap, SyncLeapIdentifier},
    NodeRng,
};
pub(crate) use error::LeapActivityError;
pub(crate) use event::Event;
pub(crate) use leap_state::LeapState;

use metrics::Metrics;

use self::leap_activity::LeapActivity;

const COMPONENT_NAME: &str = "sync_leaper";

#[derive(Clone, Debug, DataSize)]
pub(crate) enum PeerState {
    RequestSent,
    Rejected,
    CouldntFetch,
    Fetched(Box<SyncLeap>),
}
#[derive(Debug, DataSize)]
pub(crate) struct SyncLeaper {
    leap_activity: Option<LeapActivity>,
    chainspec: Arc<Chainspec>,
    #[data_size(skip)]
    metrics: Metrics,
}

impl SyncLeaper {
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(SyncLeaper {
            leap_activity: None,
            chainspec,
            metrics: Metrics::new(registry)?,
        })
    }

    // called from Reactor control logic to scrape results
    pub(crate) fn leap_status(&mut self) -> LeapState {
        match &self.leap_activity {
            None => LeapState::Idle,
            Some(activity) => {
                let result = activity.status();
                if result.active() == false {
                    match result {
                        LeapState::Received { .. } | LeapState::Failed { .. } => {
                            self.metrics
                                .sync_leap_duration
                                .observe(activity.leap_start().elapsed().as_secs_f64());
                        }
                        LeapState::Idle | LeapState::Awaiting { .. } => {
                            // should be unreachable
                            error!(status = %result, ?activity, "sync leaper has inconsistent status");
                        }
                    }
                    self.leap_activity = None;
                }
                result
            }
        }
    }

    fn register_leap_attempt<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sync_leap_identifier: SyncLeapIdentifier,
        peers_to_ask: Vec<NodeId>,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<SyncLeap>> + Send,
    {
        info!(%sync_leap_identifier, "registering leap attempt");
        let mut effects = Effects::new();
        if peers_to_ask.is_empty() {
            error!("tried to start fetching a sync leap without peers to ask");
            return effects;
        }
        if let Some(leap_activity) = self.leap_activity.as_mut() {
            if leap_activity.sync_leap_identifier() != &sync_leap_identifier {
                error!(
                    current_sync_leap_identifier = %leap_activity.sync_leap_identifier(),
                    requested_sync_leap_identifier = %sync_leap_identifier,
                    "tried to start fetching a sync leap for a different sync_leap_identifier"
                );
                return effects;
            }

            for peer in peers_to_ask {
                if false == leap_activity.peers().contains_key(&peer) {
                    effects.extend(
                        effect_builder
                            .fetch::<SyncLeap>(sync_leap_identifier, peer, self.chainspec.clone())
                            .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                                sync_leap_identifier,
                                fetch_result,
                            }),
                    );
                    leap_activity
                        .peers_mut()
                        .insert(peer, PeerState::RequestSent);
                }
            }
            return effects;
        }

        let peers = peers_to_ask
            .into_iter()
            .map(|peer| {
                effects.extend(
                    effect_builder
                        .fetch::<SyncLeap>(sync_leap_identifier, peer, self.chainspec.clone())
                        .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                            sync_leap_identifier,
                            fetch_result,
                        }),
                );
                (peer, PeerState::RequestSent)
            })
            .collect();
        self.leap_activity = Some(LeapActivity::new(
            sync_leap_identifier,
            peers,
            Instant::now(),
        ));
        effects
    }

    fn fetch_received(
        &mut self,
        sync_leap_identifier: SyncLeapIdentifier,
        fetch_result: FetchResult<SyncLeap>,
    ) {
        let leap_activity = match &mut self.leap_activity {
            Some(leap_activity) => leap_activity,
            None => {
                warn!(
                    %sync_leap_identifier,
                    "received a sync leap response while no requests were in progress"
                );
                return;
            }
        };

        if leap_activity.sync_leap_identifier() != &sync_leap_identifier {
            warn!(
                requested_hash=%leap_activity.sync_leap_identifier(),
                response_hash=%sync_leap_identifier,
                "block hash in the response doesn't match the one requested"
            );
            return;
        }

        match fetch_result {
            Ok(FetchedData::FromStorage { .. }) => {
                error!(%sync_leap_identifier, "fetched a sync leap from storage - should never happen");
            }
            Ok(FetchedData::FromPeer { item, peer, .. }) => {
                let peer_state = match leap_activity.peers_mut().get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %sync_leap_identifier,
                            "received a sync leap response from an unknown peer"
                        );
                        return;
                    }
                };
                *peer_state = PeerState::Fetched(Box::new(*item));
                self.metrics.sync_leap_fetched_from_peer.inc();
            }
            Err(fetcher::Error::Rejected { peer, .. }) => {
                let peer_state = match leap_activity.peers_mut().get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %sync_leap_identifier,
                            "received a sync leap response from an unknown peer"
                        );
                        return;
                    }
                };
                info!(%peer, %sync_leap_identifier, "peer rejected our request for a sync leap");
                *peer_state = PeerState::Rejected;
                self.metrics.sync_leap_rejected_by_peer.inc();
            }
            Err(error) => {
                let peer = error.peer();
                info!(?error, %peer, %sync_leap_identifier, "failed to fetch a sync leap from peer");
                let peer_state = match leap_activity.peers_mut().get_mut(peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %sync_leap_identifier,
                            "received a sync leap response from an unknown peer"
                        );
                        return;
                    }
                };
                *peer_state = PeerState::CouldntFetch;
                self.metrics.sync_leap_cant_fetch.inc();
            }
        }
    }
}

impl<REv> Component<REv> for SyncLeaper
where
    REv: From<FetcherRequest<SyncLeap>> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            } => self.register_leap_attempt(effect_builder, sync_leap_identifier, peers_to_ask),
            Event::FetchedSyncLeapFromPeer {
                sync_leap_identifier,
                fetch_result,
            } => {
                self.fetch_received(sync_leap_identifier, fetch_result);
                Effects::new()
            }
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
