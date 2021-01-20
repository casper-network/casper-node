use datasize::DataSize;
use rand::{seq::SliceRandom, Rng};

#[derive(DataSize, Debug)]
pub struct PeersState<I> {
    // Set of peers that we can request blocks from.
    peers: Vec<I>,
    // Peers we have not yet requested current block from.
    // NOTE: Maybe use a bitmask to decide which peers were tried?
    peers_to_try: Vec<I>,
}

impl<I: Clone + PartialEq + 'static> PeersState<I> {
    pub fn new() -> Self {
        PeersState {
            peers: Default::default(),
            peers_to_try: Default::default(),
        }
    }

    /// Resets `peers_to_try` back to all `peers` we know of.
    pub(crate) fn reset<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        self.peers_to_try = self.peers.clone();
        self.peers_to_try.as_mut_slice().shuffle(rng);
    }

    /// Returns a random peer.
    pub(crate) fn random(&mut self) -> Option<I> {
        self.peers_to_try.pop()
    }

    /// Unsafe version of `random_peer`.
    /// Panics if no peer is available for querying.
    pub(crate) fn random_unsafe(&mut self) -> I {
        self.random().expect("At least one peer available.")
    }

    /// Peer misbehaved (returned us invalid data).
    /// Removes it from the set of nodes we request data from.
    pub(crate) fn ban(&mut self, peer: I) {
        let index = self.peers.iter().position(|p| *p == peer);
        index.map(|idx| self.peers.remove(idx));
    }

    /// Returns whether known peer set is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Adds a new peer.
    pub(crate) fn push(&mut self, peer: I) {
        self.peers.push(peer)
    }
}
