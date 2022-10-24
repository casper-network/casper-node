use std::collections::{btree_map::Entry, BTreeMap};

use datasize::DataSize;
use itertools::Itertools;
use rand::seq::IteratorRandom;

use crate::{types::NodeId, NodeRng};
use casper_types::{TimeDiff, Timestamp};

#[derive(Copy, Clone, PartialEq, Eq, DataSize, Debug, Default)]
enum PeerQuality {
    #[default]
    Unknown,
    Unresponsive,
    Unreliable,
    Reliable,
    Dishonest,
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct PeerList {
    peer_list: BTreeMap<NodeId, PeerQuality>,
    latch: Timestamp,
    max_simultaneous_peers: u32,
    peer_refresh_interval: TimeDiff,
}

impl PeerList {
    pub(super) fn new(max_simultaneous_peers: u32, peer_refresh_interval: TimeDiff) -> Self {
        PeerList {
            peer_list: BTreeMap::new(),
            latch: Timestamp::now(),
            max_simultaneous_peers,
            peer_refresh_interval,
        }
    }
    pub(super) fn register_peer(&mut self, peer: NodeId) {
        if self.peer_list.contains_key(&peer) {
            return;
        }
        self.peer_list.insert(peer, PeerQuality::Unknown);
        self.latch = Timestamp::now();
    }

    pub(super) fn dishonest_peers(&self) -> Vec<NodeId> {
        self.peer_list
            .iter()
            .filter_map(|(node_id, pq)| {
                if *pq == PeerQuality::Dishonest {
                    Some(*node_id)
                } else {
                    None
                }
            })
            .collect_vec()
    }

    pub(super) fn flush(&mut self) {
        self.peer_list.clear();
    }

    pub(super) fn flush_dishonest_peers(&mut self) {
        self.peer_list.retain(|_, v| *v != PeerQuality::Dishonest);
        self.latch = Timestamp::now();
    }

    pub(super) fn disqualify_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            self.peer_list.insert(peer_id, PeerQuality::Dishonest);
            self.latch = Timestamp::now();
        }
    }

    pub(super) fn promote_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            // vacant should be unreachable
            match self.peer_list.entry(peer_id) {
                Entry::Vacant(_) => {
                    self.peer_list.insert(peer_id, PeerQuality::Unknown);
                }
                Entry::Occupied(entry) => match entry.get() {
                    PeerQuality::Dishonest => {
                        // no change -- this is terminal
                    }
                    PeerQuality::Unknown => {
                        self.peer_list.insert(peer_id, PeerQuality::Unreliable);
                    }
                    PeerQuality::Unresponsive => {
                        self.peer_list.insert(peer_id, PeerQuality::Unreliable);
                    }
                    PeerQuality::Unreliable => {
                        self.peer_list.insert(peer_id, PeerQuality::Reliable);
                    }
                    PeerQuality::Reliable => {
                        // no change -- this is the top
                    }
                },
            }
        }
    }

    pub(super) fn demote_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            // vacant should be unreachable
            match self.peer_list.entry(peer_id) {
                Entry::Vacant(_) => {
                    // no change
                }
                Entry::Occupied(entry) => match entry.get() {
                    PeerQuality::Dishonest | PeerQuality::Unknown => {
                        // no change
                    }
                    PeerQuality::Unresponsive => {
                        self.peer_list.insert(peer_id, PeerQuality::Unknown);
                    }
                    PeerQuality::Unreliable => {
                        self.peer_list.insert(peer_id, PeerQuality::Unresponsive);
                    }
                    PeerQuality::Reliable => {
                        self.peer_list.insert(peer_id, PeerQuality::Unreliable);
                    }
                },
            }
        }
    }

    pub(super) fn need_peers(&self) -> bool {
        self.peer_list.is_empty()

        // if self.peer_list.is_empty() {
        //     return true;
        // }
        // // periodically ask for refreshed peers
        // // NOTE: if we decide to do this imperatively from the reactor, this can likely be
        // removed if Timestamp::now().saturating_diff(self.latch) >
        // self.peer_refresh_interval {     error!("XXXXX - need_next: Peers: list is NOT
        // empty, but latch diff {} is telling us we need to refresh",
        // Timestamp::now().saturating_diff(self.latch));     return true;
        // }
        // // if reliable / untried peer count is below self.simultaneous_peers, ask for new peers
        // let reliability_goal = self.max_simultaneous_peers as usize;
        // error!("XXXXX - reliability goal is {}", reliability_goal);
        // let good_peer_count = self.peer_list
        // .iter()
        // .filter(|(_, pq)| **pq == PeerQuality::Reliable || **pq == PeerQuality::Unknown)
        // .count();
        // error!("XXXXX - good peer count is {}", good_peer_count);
        // self.peer_list
        //     .iter()
        //     .filter(|(_, pq)| **pq == PeerQuality::Reliable || **pq == PeerQuality::Unknown)
        //     .count()
        //     < reliability_goal
    }

    pub(super) fn qualified_peers(&self, rng: &mut NodeRng) -> Vec<NodeId> {
        let up_to = self.max_simultaneous_peers as usize;

        // get most useful up to limit
        let mut peers: Vec<NodeId> = self
            .peer_list
            .iter()
            .filter(|(_peer, quality)| **quality == PeerQuality::Reliable)
            .choose_multiple(rng, up_to)
            .into_iter()
            .map(|(peer, _)| *peer)
            .collect();

        // if below limit get semi-useful
        let missing = up_to.saturating_sub(peers.len());
        if missing > 0 {
            let better_than_nothing = self
                .peer_list
                .iter()
                .filter(|(_peer, quality)| {
                    **quality == PeerQuality::Unreliable || **quality == PeerQuality::Unknown
                })
                .choose_multiple(rng, missing)
                .into_iter()
                .map(|(peer, _)| *peer);

            peers.extend(better_than_nothing);
        }

        peers
    }
}
