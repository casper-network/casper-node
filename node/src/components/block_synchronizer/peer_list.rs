#[cfg(test)]
mod tests;

use std::collections::{btree_map::Entry, BTreeMap};

use datasize::DataSize;
use itertools::Itertools;
use rand::seq::IteratorRandom;
use tracing::debug;

use crate::{types::NodeId, NodeRng};
use casper_types::{TimeDiff, Timestamp};

#[derive(Copy, Clone, PartialEq, Eq, DataSize, Debug, Default)]
enum PeerQuality {
    #[default]
    Unknown,
    Unreliable,
    Reliable,
    Dishonest,
}

pub(super) enum PeersStatus {
    Sufficient,
    Insufficient,
    Stale,
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct PeerList {
    peer_list: BTreeMap<NodeId, PeerQuality>,
    keep_fresh: Timestamp,
    max_simultaneous_peers: u32,
    peer_refresh_interval: TimeDiff,
}

impl PeerList {
    pub(super) fn new(max_simultaneous_peers: u32, peer_refresh_interval: TimeDiff) -> Self {
        PeerList {
            peer_list: BTreeMap::new(),
            keep_fresh: Timestamp::now(),
            max_simultaneous_peers,
            peer_refresh_interval,
        }
    }
    pub(super) fn register_peer(&mut self, peer: NodeId) {
        if self.peer_list.contains_key(&peer) {
            return;
        }
        self.peer_list.insert(peer, PeerQuality::Unknown);
        self.keep_fresh = Timestamp::now();
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
    }

    pub(super) fn disqualify_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            self.peer_list.insert(peer_id, PeerQuality::Dishonest);
        }
    }

    pub(super) fn promote_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            debug!("BlockSynchronizer: promoting peer {:?}", peer_id);
            // vacant should be unreachable
            match self.peer_list.entry(peer_id) {
                Entry::Vacant(_) => {
                    self.peer_list.insert(peer_id, PeerQuality::Unknown);
                }
                Entry::Occupied(entry) => match entry.get() {
                    PeerQuality::Dishonest => {
                        // no change -- this is terminal
                    }
                    PeerQuality::Unreliable | PeerQuality::Unknown => {
                        self.peer_list.insert(peer_id, PeerQuality::Reliable);
                    }
                    PeerQuality::Reliable => {
                        // no change -- this is the best
                    }
                },
            }
        }
    }

    pub(super) fn demote_peer(&mut self, peer: Option<NodeId>) {
        if let Some(peer_id) = peer {
            debug!("BlockSynchronizer: demoting peer {:?}", peer_id);
            // vacant should be unreachable
            match self.peer_list.entry(peer_id) {
                Entry::Vacant(_) => {
                    // no change
                }
                Entry::Occupied(entry) => match entry.get() {
                    PeerQuality::Dishonest | PeerQuality::Unreliable => {
                        // no change
                    }
                    PeerQuality::Reliable | PeerQuality::Unknown => {
                        self.peer_list.insert(peer_id, PeerQuality::Unreliable);
                    }
                },
            }
        }
    }

    pub(super) fn need_peers(&mut self) -> PeersStatus {
        if !self
            .peer_list
            .iter()
            .any(|(_, pq)| *pq != PeerQuality::Dishonest)
        {
            debug!("PeerList: no honest peers");
            return PeersStatus::Insufficient;
        }

        // periodically ask for refreshed peers
        if Timestamp::now().saturating_diff(self.keep_fresh) > self.peer_refresh_interval {
            self.keep_fresh = Timestamp::now();
            let count = self
                .peer_list
                .iter()
                .filter(|(_, pq)| **pq == PeerQuality::Reliable || **pq == PeerQuality::Unknown)
                .count();
            let reliability_goal = self.max_simultaneous_peers as usize;
            if count < reliability_goal {
                debug!("PeerList: is stale");
                return PeersStatus::Stale;
            }
        }

        PeersStatus::Sufficient
    }

    fn get_random_peers_by_quality(
        &self,
        rng: &mut NodeRng,
        up_to: usize,
        peer_quality: PeerQuality,
    ) -> Vec<NodeId> {
        self.peer_list
            .iter()
            .filter(|(_peer, quality)| **quality == peer_quality)
            .choose_multiple(rng, up_to)
            .into_iter()
            .map(|(peer, _)| *peer)
            .collect()
    }

    pub(super) fn qualified_peers(&self, rng: &mut NodeRng) -> Vec<NodeId> {
        self.qualified_peers_up_to(rng, self.max_simultaneous_peers as usize)
    }

    pub(super) fn qualified_peers_up_to(&self, rng: &mut NodeRng, up_to: usize) -> Vec<NodeId> {
        // get most useful up to limit
        let mut peers = self.get_random_peers_by_quality(rng, up_to, PeerQuality::Reliable);

        // if below limit get unknown peers which may or may not be useful
        let missing = up_to.saturating_sub(peers.len());
        if missing > 0 {
            peers.extend(self.get_random_peers_by_quality(rng, missing, PeerQuality::Unknown));
        }

        // if still below limit try unreliable peers again until we have the chance to refresh the
        // peer list
        let missing = up_to.saturating_sub(peers.len());
        if missing > 0 {
            peers.extend(self.get_random_peers_by_quality(rng, missing, PeerQuality::Unreliable));
        }

        peers
    }
}
