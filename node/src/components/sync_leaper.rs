//! The Sync Leaper
mod error;
mod event;

use std::{cmp::Ordering, collections::HashMap};

use datasize::DataSize;
use num_rational::Ratio;
use tracing::{error, info, warn};

use crate::{
    components::{
        fetcher::{self, FetchResult, FetchedData},
        Component,
    },
    effect::{requests::FetcherRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockHash, NodeId, SyncLeap},
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
        block_hash: BlockHash,
        in_flight: usize,
    },
    Received {
        best_available: Box<SyncLeap>,
        from_peers: Vec<NodeId>,
        in_flight: usize,
    },
    Failed {
        block_hash: BlockHash,
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
    block_hash: BlockHash,
    peers: HashMap<NodeId, PeerState>,
}

impl LeapActivity {
    fn status(&self) -> LeapStatus {
        let block_hash = self.block_hash;
        let in_flight = self
            .peers
            .values()
            .filter(|state| matches!(state, PeerState::RequestSent))
            .count();
        let responsed = self.peers.len() - in_flight;
        if in_flight == 0 && responsed == 0 {
            return LeapStatus::Failed {
                block_hash,
                in_flight,
                error: LeapActivityError::NoPeers(block_hash),
                from_peers: vec![],
            };
        }

        if in_flight > 0 && responsed == 0 {
            return LeapStatus::Awaiting {
                block_hash,
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
                block_hash,
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
                PeerState::Fetched(sync_leap) => {
                    let height = sync_leap.highest_block_height();
                    match &maybe_ret {
                        None => {
                            maybe_ret = Some(sync_leap);
                            peers.push(*peer);
                        }
                        Some(current_ret) => {
                            match current_ret.highest_block_height().cmp(&height) {
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
                    }
                }
                PeerState::RequestSent | PeerState::Rejected | PeerState::CouldntFetch => {}
            }
        }

        match maybe_ret {
            Some(sync_leap) => Ok((*sync_leap.clone(), peers)),
            None => {
                if reject_count > 0 {
                    Err(LeapActivityError::TooOld(self.block_hash, peers))
                } else {
                    Err(LeapActivityError::Unobtainable(self.block_hash, peers))
                }
            }
        }
    }
}

#[derive(Debug, DataSize)]
pub(crate) struct SyncLeaper {
    leap_activity: Option<LeapActivity>,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
}

impl SyncLeaper {
    pub(crate) fn new(finality_threshold_fraction: Ratio<u64>) -> SyncLeaper {
        SyncLeaper {
            leap_activity: None,
            finality_threshold_fraction,
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
        block_hash: BlockHash,
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
            if leap_activity.block_hash != block_hash {
                error!(
                    current_trusted_hash = %leap_activity.block_hash,
                    requested_trusted_hash = %block_hash,
                    "tried to start fetching a sync leap for a different trusted hash"
                );
                return effects;
            }

            for peer in peers_to_ask {
                if false == leap_activity.peers.contains_key(&peer) {
                    effects.extend(
                        effect_builder
                            .fetch::<SyncLeap>(
                                block_hash,
                                peer,
                                self.finality_threshold_fraction.into(),
                            )
                            .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                                block_hash,
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
                        .fetch::<SyncLeap>(
                            block_hash,
                            peer,
                            self.finality_threshold_fraction.into(),
                        )
                        .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                            block_hash,
                            fetch_result,
                        }),
                );
                (peer, PeerState::RequestSent)
            })
            .collect();
        self.leap_activity = Some(LeapActivity { block_hash, peers });
        effects
    }

    fn fetch_received(
        &mut self,
        block_hash: BlockHash,
        fetch_result: FetchResult<SyncLeap>,
    ) -> Effects<Event> {
        let effects = Effects::new();
        let leap_activity = match &mut self.leap_activity {
            Some(leap_activity) => leap_activity,
            None => {
                warn!(
                    %block_hash,
                    "received a sync leap response while no requests were in progress"
                );
                return effects;
            }
        };

        if leap_activity.block_hash != block_hash {
            warn!(
                requested_hash=%leap_activity.block_hash,
                response_hash=%block_hash,
                "block hash in the response doesn't match the one requested"
            );
            return effects;
        }

        match fetch_result {
            Ok(FetchedData::FromStorage { .. }) => {
                error!(%block_hash, "fetched a sync leap from storage - should never happen");
                return Effects::new();
            }
            Ok(FetchedData::FromPeer { item, peer, .. }) => {
                let peer_state = match leap_activity.peers.get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %block_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return Effects::new();
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
                            %block_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return effects;
                    }
                };
                info!(%peer, %block_hash, "peer rejected our request for a sync leap");
                *peer_state = PeerState::Rejected;
            }
            Err(error) => {
                let peer = error.peer();
                info!(?error, %peer, %block_hash, "failed to fetch a sync leap from peer");
                let peer_state = match leap_activity.peers.get_mut(peer) {
                    Some(state) => state,
                    None => {
                        warn!(
                            ?peer,
                            %block_hash,
                            "received a sync leap response from an unknown peer"
                        );
                        return effects;
                    }
                };
                *peer_state = PeerState::CouldntFetch;
            }
        }
        effects
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
                block_hash: trusted_hash,
                peers_to_ask,
            } => self.register_leap_attempt(effect_builder, trusted_hash, peers_to_ask),
            Event::FetchedSyncLeapFromPeer {
                block_hash: trusted_hash,
                fetch_result,
            } => self.fetch_received(trusted_hash, fetch_result),
        }
    }
}
