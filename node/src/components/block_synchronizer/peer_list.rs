use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::time::Duration;

use datasize::DataSize;
use itertools::Itertools;
use rand::{prelude::SliceRandom, seq::IteratorRandom, Rng};

use crate::types::NodeId;
use crate::NodeRng;
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
    simultaneous_peers: u8,
}

impl PeerList {
    pub(super) fn new(simultaneous_peers: u8) -> Self {
        PeerList {
            peer_list: BTreeMap::new(),
            latch: Timestamp::now(),
            simultaneous_peers,
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
        self.peer_list.retain(|k, v| *v != PeerQuality::Dishonest);
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
    // TODO: add config for PEER_REFRESH_INTERVAL
    const PEER_REFRESH_INTERVAL: u32 = 90;
    pub(super) fn need_peers(&self) -> bool {
        if self.peer_list.is_empty() {
            return true;
        }
        // periodically ask for refreshed peers
        // NOTE: if we decide to do this imperatively from the reactor, this can likely be removed
        if Timestamp::now().saturating_diff(self.latch)
            > TimeDiff::from_seconds(PeerList::PEER_REFRESH_INTERVAL)
        {
            return true;
        }
        // if reliable / untried peer count is below self.simultaneous_peers, ask for new peers
        let reliability_goal = self.simultaneous_peers as usize;
        self.peer_list
            .iter()
            .filter(|(_, pq)| **pq == PeerQuality::Reliable || **pq == PeerQuality::Unknown)
            .collect_vec()
            .len()
            < reliability_goal
    }

    pub(super) fn qualified_peers(&self, rng: &mut NodeRng) -> Vec<NodeId> {
        let up_to = self.simultaneous_peers;

        // get most useful up to limit
        let mut peers: Vec<NodeId> = self
            .peer_list
            .iter()
            .filter(|(k, v)| **v == PeerQuality::Reliable)
            .choose_multiple(rng, up_to as usize)
            .into_iter()
            .map(|(k, _)| *k)
            .collect();

        // if below limit get semi-useful
        let missing: usize = peers.len().saturating_sub(up_to as usize);
        if missing > 0 {
            let mut better_than_nothing = self
                .peer_list
                .iter()
                .filter(|(_, v)| **v == PeerQuality::Unreliable || **v == PeerQuality::Unknown)
                .choose_multiple(rng, missing)
                .into_iter()
                .map(|(k, _)| *k)
                .collect_vec();

            peers.append(&mut better_than_nothing);
        }

        peers
    }
}
