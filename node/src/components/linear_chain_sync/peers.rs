use std::collections::VecDeque;

use datasize::DataSize;
use rand::{seq::SliceRandom, Rng};

#[derive(DataSize, Debug)]
pub struct PeersState<I> {
    // Set of peers that we can request blocks from.
    peers: Vec<I>,
    // Peers we have not yet requested current block from.
    // NOTE: Maybe use a bitmask to decide which peers were tried?
    peers_to_try: Vec<I>,
    // Peers we successfully downloaded data from previously.
    // Have higher chance of having the next data.
    succ_peers: VecDeque<I>,
    succ_attempts: u8,
    succ_attempts_max: u8,
}

impl<I: Clone + PartialEq + 'static> PeersState<I> {
    pub fn new() -> Self {
        PeersState {
            peers: Default::default(),
            peers_to_try: Default::default(),
            succ_peers: Default::default(),
            succ_attempts: 0,
            succ_attempts_max: 5,
        }
    }

    /// Resets `peers_to_try` back to all `peers` we know of.
    pub(crate) fn reset<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        self.peers_to_try = self.peers.clone();
        self.peers_to_try.as_mut_slice().shuffle(rng);
    }

    /// Returns a random peer.
    pub(crate) fn random(&mut self) -> Option<I> {
        if self.succ_attempts < self.succ_attempts_max {
            self.next_succ().or_else(|| self.peers_to_try.pop())
        } else {
            self.succ_attempts = 0;
            self.peers_to_try.pop().or_else(|| self.next_succ())
        }
    }

    /// Unsafe version of `random_peer`.
    /// Panics if no peer is available for querying.
    pub(crate) fn random_unsafe(&mut self) -> I {
        self.random().expect("At least one peer available.")
    }

    /// Peer misbehaved (returned us invalid data).
    /// Remove it from the set of nodes we request data from.
    pub(crate) fn ban(&mut self, peer: &I) {
        self.peers.retain(|p| p != peer);
        self.succ_peers.retain(|p| p != peer);
    }

    /// Adds a new peer.
    pub(crate) fn push(&mut self, peer: I) {
        self.peers.push(peer)
    }

    /// Returns the next peer, if any, that we downloaded data the previous time.
    /// Keeps the peer in the set of `succ_peers`.
    fn next_succ(&mut self) -> Option<I> {
        let peer = self.succ_peers.pop_front()?;
        self.succ_peers.push_back(peer.clone());
        Some(peer)
    }

    /// Peer didn't respond or didn't have the data we asked for.
    pub(crate) fn failure(&mut self, peer: &I) {
        self.succ_peers.retain(|id| id != peer);
    }

    /// Peer had the data we asked for.
    pub(crate) fn success(&mut self, peer: I) {
        self.succ_attempts += 1;
        self.succ_peers.push_back(peer);
    }
}
