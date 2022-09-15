#![allow(unused)] // TODO: To be removed

//! The Sync Leaper
mod error;

use std::{collections::HashMap, convert::Infallible};

use num_rational::Ratio;
use tracing::{error, info, warn};

use crate::{
    components::{
        fetcher::{self, FetchResult, FetchedData},
        Component,
    },
    effect::{
        requests::{FetcherRequest, SyncLeapRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{BlockHash, NodeId, SyncLeap},
    NodeRng,
};

pub(crate) use error::{ConstructSyncLeapError, PullSyncLeapError};

pub(crate) enum Event {
    SyncLeapRequest(SyncLeapRequest),
    StartPullingSyncLeap {
        trusted_hash: BlockHash,
        peers_to_ask: Vec<NodeId>,
    },
    FetchedSyncLeapFromPeer {
        trusted_hash: BlockHash,
        fetch_result: FetchResult<SyncLeap>,
    },
}

pub(crate) struct SyncLeaper {
    maybe_pull_request_in_progress: Option<PullRequestInProgress>,
    finality_threshold_fraction: Ratio<u64>,
}

struct PullRequestInProgress {
    trusted_hash: BlockHash,
    peers: HashMap<NodeId, PeerState>,
}

impl PullRequestInProgress {
    fn poll_response(&self) -> PullRequestResult {
        let num_in_flight = self
            .peers
            .values()
            .filter(|state| matches!(state, PeerState::RequestSent))
            .count();
        let num_responded = self.peers.len() - num_in_flight;
        if num_in_flight == 0 && num_responded == 0 {
            PullRequestResult::NotPulling
        } else if num_in_flight > 0 && num_responded == 0 {
            PullRequestResult::AwaitingResponses
        } else if num_in_flight > 0 && num_responded > 0 {
            PullRequestResult::PartialResponse(self.best_response())
        } else {
            PullRequestResult::FinalResponse(self.best_response())
        }
    }

    fn best_response(&self) -> Result<SyncLeap, PullSyncLeapError> {
        let sync_leaps: Vec<_> = self
            .peers
            .values()
            .filter_map(|peer_state| match peer_state {
                PeerState::Fetched(sync_leap) => Some(sync_leap.clone()),
                _ => None,
            })
            .collect();
        let reject_count = self
            .peers
            .values()
            .filter(|peer_state| matches!(peer_state, PeerState::Rejected))
            .count();

        sync_leaps
            .into_iter()
            .max_by_key(|sync_leap| sync_leap.highest_era())
            .ok_or_else(|| {
                // sync_leaps is empty, so our best response is either that the trusted hash was
                // too old or that we just couldn't fetch a SyncLeap
                if reject_count > 0 {
                    PullSyncLeapError::TrustedHashTooOld(self.trusted_hash)
                } else {
                    PullSyncLeapError::CouldntFetch(self.trusted_hash)
                }
            })
    }
}

pub(crate) enum PullRequestResult {
    NotPulling,
    AwaitingResponses,
    PartialResponse(Result<SyncLeap, PullSyncLeapError>),
    FinalResponse(Result<SyncLeap, PullSyncLeapError>),
}

enum PeerState {
    RequestSent,
    Rejected,
    CouldntFetch,
    Fetched(SyncLeap),
}

impl SyncLeaper {
    pub(crate) fn new(finality_threshold_fraction: Ratio<u64>) -> SyncLeaper {
        SyncLeaper {
            maybe_pull_request_in_progress: None,
            finality_threshold_fraction,
        }
    }

    pub(crate) fn poll_response(&mut self) -> PullRequestResult {
        match &self.maybe_pull_request_in_progress {
            None => PullRequestResult::NotPulling,
            Some(pull_request_in_progress) => {
                let result = pull_request_in_progress.poll_response();
                match result {
                    PullRequestResult::NotPulling => {
                        error!("a sync leap pull request was constructed without peers to ask");
                        self.maybe_pull_request_in_progress = None;
                    }
                    PullRequestResult::FinalResponse(_) => {
                        self.maybe_pull_request_in_progress = None;
                    }
                    // nothing to do for other variants
                    _ => (),
                }
                result
            }
        }
    }

    fn handle_pull_request<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trusted_hash: BlockHash,
        peers_to_ask: Vec<NodeId>,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<SyncLeap>> + Send,
    {
        if peers_to_ask.is_empty() {
            error!("tried to start fetching a sync leap without peers to ask");
            return Effects::new();
        }
        if let Some(_) = &self.maybe_pull_request_in_progress {
            // TODO: if the trusted hash is the same, maybe another component is just giving us
            // more peers? consider handling that in a different way
            return Effects::new();
        }
        let mut effects = Effects::new();
        let peers = peers_to_ask
            .into_iter()
            .map(|peer| {
                effects.extend(
                    effect_builder
                        .fetch::<SyncLeap>(trusted_hash, peer, self.finality_threshold_fraction)
                        .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                            trusted_hash,
                            fetch_result,
                        }),
                );
                // note the peer states
                (peer, PeerState::RequestSent)
            })
            .collect();
        self.maybe_pull_request_in_progress = Some(PullRequestInProgress {
            trusted_hash,
            peers,
        });
        effects
    }

    fn handle_construct_request<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trusted_hash: BlockHash,
        responder: Responder<Result<SyncLeap, ConstructSyncLeapError>>,
    ) -> Effects<Event> {
        Effects::new()
    }

    fn handle_fetched_sync_leap(
        &mut self,
        trusted_hash: BlockHash,
        fetch_result: FetchResult<SyncLeap>,
    ) -> Effects<Event> {
        let request_in_progress = match &mut self.maybe_pull_request_in_progress {
            Some(request_in_progress) => request_in_progress,
            None => {
                warn!(
                    %trusted_hash,
                    "received a sync leap response while no requests were in progress"
                );
                return Effects::new();
            }
        };

        if request_in_progress.trusted_hash != trusted_hash {
            warn!(
                requested_hash=%request_in_progress.trusted_hash,
                response_hash=%trusted_hash,
                "trusted hash in the response doesn't match the one requested"
            );
            return Effects::new();
        }

        match fetch_result {
            Ok(FetchedData::FromStorage { .. }) => {
                error!(%trusted_hash, "fetched a sync leap from storage - should never happen");
                return Effects::new();
            }
            Ok(FetchedData::FromPeer { item, peer, .. }) => {
                let peer_state = match request_in_progress.peers.get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %trusted_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return Effects::new();
                    }
                };
                *peer_state = PeerState::Fetched(*item);
            }
            Err(fetcher::Error::Rejected { peer, .. }) => {
                let peer_state = match request_in_progress.peers.get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %trusted_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return Effects::new();
                    }
                };
                info!(%peer, %trusted_hash, "peer rejected our request for a sync leap");
                *peer_state = PeerState::Rejected;
            }
            Err(error) => {
                let peer = error.peer();
                info!(?error, %peer, %trusted_hash, "failed to fetch a sync leap from peer");
                let peer_state = match request_in_progress.peers.get_mut(peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %trusted_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return Effects::new();
                    }
                };
                *peer_state = PeerState::CouldntFetch;
            }
        }

        Effects::new()
    }
}

impl<REv> Component<REv> for SyncLeaper
where
    REv: From<FetcherRequest<SyncLeap>> + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::SyncLeapRequest(SyncLeapRequest {
                trusted_hash,
                responder,
            }) => self.handle_construct_request(effect_builder, trusted_hash, responder),
            Event::StartPullingSyncLeap {
                trusted_hash,
                peers_to_ask,
            } => self.handle_pull_request(effect_builder, trusted_hash, peers_to_ask),
            Event::FetchedSyncLeapFromPeer {
                trusted_hash,
                fetch_result,
            } => self.handle_fetched_sync_leap(trusted_hash, fetch_result),
        }
    }
}
