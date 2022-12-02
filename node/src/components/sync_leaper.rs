//! The Sync Leaper
mod error;
mod event;

use std::{cmp::Ordering, collections::HashMap, sync::Arc};

use datasize::DataSize;
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

#[derive(Debug, DataSize)]
enum PeerState {
    RequestSent,
    Rejected,
    CouldntFetch,
    Fetched(Box<SyncLeap>),
}

#[derive(Debug, DataSize)]
pub(crate) enum LeapStatus {
    Inactive,
    Awaiting {
        sync_leap_identifier: SyncLeapIdentifier,
        in_flight: usize,
    },
    Received {
        best_available: Box<SyncLeap>,
        from_peers: Vec<NodeId>,
        in_flight: usize,
    },
    Failed {
        sync_leap_identifier: SyncLeapIdentifier,
        error: LeapActivityError,
        from_peers: Vec<NodeId>,
        in_flight: usize,
    },
}

impl LeapStatus {
    fn in_flight(&self) -> usize {
        match self {
            LeapStatus::Inactive => 0,
            LeapStatus::Awaiting { in_flight, .. }
            | LeapStatus::Received { in_flight, .. }
            | LeapStatus::Failed { in_flight, .. } => *in_flight,
        }
    }

    fn active(&self) -> bool {
        self.in_flight() > 0
    }
}

#[derive(Debug, DataSize)]
struct LeapActivity {
    sync_leap_identifier: SyncLeapIdentifier,
    peers: HashMap<NodeId, PeerState>,
}

impl LeapActivity {
    fn status(&self) -> LeapStatus {
        let sync_leap_identifier = self.sync_leap_identifier;
        let in_flight = self
            .peers
            .values()
            .filter(|state| matches!(state, PeerState::RequestSent))
            .count();
        let responsed = self.peers.len() - in_flight;
        if in_flight == 0 && responsed == 0 {
            return LeapStatus::Failed {
                sync_leap_identifier,
                in_flight,
                error: LeapActivityError::NoPeers(sync_leap_identifier),
                from_peers: vec![],
            };
        }
        if in_flight > 0 && responsed == 0 {
            return LeapStatus::Awaiting {
                sync_leap_identifier,
                in_flight,
            };
        }
        match self.best_response() {
            Ok((best_available, from_peers)) => LeapStatus::Received {
                in_flight,
                best_available: Box::new(best_available),
                from_peers,
            },
            Err(error) => LeapStatus::Failed {
                sync_leap_identifier,
                from_peers: vec![],
                in_flight,
                error,
            },
        }
    }

    fn best_response(&self) -> Result<(SyncLeap, Vec<NodeId>), LeapActivityError> {
        let reject_count = self
            .peers
            .values()
            .filter(|peer_state| matches!(peer_state, PeerState::Rejected))
            .count();

        let mut peers = vec![];
        let mut maybe_ret: Option<&Box<SyncLeap>> = None;
        for (peer, peer_state) in &self.peers {
            match peer_state {
                PeerState::Fetched(sync_leap) => match &maybe_ret {
                    None => {
                        maybe_ret = Some(sync_leap);
                        peers.push(*peer);
                    }
                    Some(current_ret) => {
                        match current_ret
                            .highest_block_height()
                            .cmp(&sync_leap.highest_block_height())
                        {
                            Ordering::Less => {
                                maybe_ret = Some(sync_leap);
                                peers = vec![*peer];
                            }
                            Ordering::Equal => {
                                peers.push(*peer);
                            }
                            Ordering::Greater => {}
                        }
                    }
                },
                PeerState::RequestSent | PeerState::Rejected | PeerState::CouldntFetch => {}
            }
        }

        match maybe_ret {
            Some(sync_leap) => Ok((*sync_leap.clone(), peers)),
            None => {
                if reject_count > 0 {
                    Err(LeapActivityError::TooOld(self.sync_leap_identifier, peers))
                } else {
                    Err(LeapActivityError::Unobtainable(
                        self.sync_leap_identifier,
                        peers,
                    ))
                }
            }
        }
    }
}

#[derive(Debug, DataSize)]
pub(crate) struct SyncLeaper {
    leap_activity: Option<LeapActivity>,
    chainspec: Arc<Chainspec>,
}

impl SyncLeaper {
    pub(crate) fn new(chainspec: Arc<Chainspec>) -> SyncLeaper {
        SyncLeaper {
            leap_activity: None,
            chainspec,
        }
    }

    // called from Reactor control logic to scrape results
    pub(crate) fn leap_status(&mut self) -> LeapStatus {
        match &self.leap_activity {
            None => LeapStatus::Inactive,
            Some(activity) => {
                let result = activity.status();
                if result.active() == false {
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
        let mut effects = Effects::new();
        if peers_to_ask.is_empty() {
            error!("tried to start fetching a sync leap without peers to ask");
            return effects;
        }
        if let Some(leap_activity) = self.leap_activity.as_mut() {
            if leap_activity.sync_leap_identifier != sync_leap_identifier {
                error!(
                    current_sync_leap_identifier = %leap_activity.sync_leap_identifier,
                    requested_sync_leap_identifier = %sync_leap_identifier,
                    "tried to start fetching a sync leap for a different sync_leap_identifier"
                );
                return effects;
            }

            for peer in peers_to_ask {
                if false == leap_activity.peers.contains_key(&peer) {
                    effects.extend(
                        effect_builder
                            .fetch::<SyncLeap>(sync_leap_identifier, peer, self.chainspec.clone())
                            .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                                sync_leap_identifier,
                                fetch_result,
                            }),
                    );
                    leap_activity.peers.insert(peer, PeerState::RequestSent);
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
        self.leap_activity = Some(LeapActivity {
            sync_leap_identifier,
            peers,
        });
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

        if leap_activity.sync_leap_identifier != sync_leap_identifier {
            warn!(
                requested_hash=%leap_activity.sync_leap_identifier,
                response_hash=%sync_leap_identifier,
                "block hash in the response doesn't match the one requested"
            );
            return;
        }

        match fetch_result {
            Ok(FetchedData::FromStorage { .. }) => {
                error!(%sync_leap_identifier, "fetched a sync leap from storage - should never happen");
                return;
            }
            Ok(FetchedData::FromPeer { item, peer, .. }) => {
                let peer_state = match leap_activity.peers.get_mut(&peer) {
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
            }
            Err(fetcher::Error::Rejected { peer, .. }) => {
                let peer_state = match leap_activity.peers.get_mut(&peer) {
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
            }
            Err(error) => {
                let peer = error.peer();
                info!(?error, %peer, %sync_leap_identifier, "failed to fetch a sync leap from peer");
                let peer_state = match leap_activity.peers.get_mut(peer) {
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
}
