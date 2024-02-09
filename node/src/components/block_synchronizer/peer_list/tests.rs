use std::collections::HashSet;

use super::*;
use casper_types::testing::TestRng;

impl PeerList {
    pub(crate) fn is_peer_unreliable(&self, peer_id: &NodeId) -> bool {
        *self.peer_list.get(peer_id).unwrap() == PeerQuality::Unreliable
    }

    pub(crate) fn is_peer_reliable(&self, peer_id: &NodeId) -> bool {
        *self.peer_list.get(peer_id).unwrap() == PeerQuality::Reliable
    }

    pub(crate) fn is_peer_unknown(&self, peer_id: &NodeId) -> bool {
        *self.peer_list.get(peer_id).unwrap() == PeerQuality::Unknown
    }
}

// Create multiple random peers
fn random_peers(rng: &mut TestRng, num_random_peers: usize) -> HashSet<NodeId> {
    (0..num_random_peers).map(|_| NodeId::random(rng)).collect()
}

#[test]
fn number_of_qualified_peers_is_correct() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));

    let test_peers: Vec<NodeId> = random_peers(&mut rng, 10).into_iter().collect();

    // Add test peers to the peer list and check the internal size
    for peer in test_peers.iter() {
        peer_list.register_peer(*peer);
    }
    assert_eq!(peer_list.peer_list.len(), 10);

    // All peers should be `Unknown`; check that the number of qualified peers is within the
    // `max_simultaneous_peers`
    let qualified_peers = peer_list.qualified_peers(&mut rng);
    assert_eq!(qualified_peers.len(), 5);

    // Promote some peers to make them `Reliable`; check the count again
    for peer in &test_peers[..3] {
        peer_list.promote_peer(*peer);
    }
    let qualified_peers = peer_list.qualified_peers(&mut rng);
    assert_eq!(qualified_peers.len(), 5);

    // Demote some peers to make them `Unreliable`; check the count again
    for peer in &test_peers[5..] {
        peer_list.demote_peer(*peer);
    }
    let qualified_peers = peer_list.qualified_peers(&mut rng);
    assert_eq!(qualified_peers.len(), 5);

    // Disqualify 7 peers; only 3 peers should remain valid for proposal
    for peer in &test_peers[..7] {
        peer_list.disqualify_peer(*peer);
    }
    let qualified_peers = peer_list.qualified_peers(&mut rng);
    assert_eq!(qualified_peers.len(), 3);
}

#[test]
fn unknown_peer_becomes_reliable_when_promoted() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));
    let test_peer = NodeId::random(&mut rng);

    peer_list.register_peer(test_peer);
    assert!(peer_list.is_peer_unknown(&test_peer));
    peer_list.promote_peer(test_peer);
    assert!(peer_list.is_peer_reliable(&test_peer));
}

#[test]
fn unknown_peer_becomes_unreliable_when_demoted() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));
    let test_peer = NodeId::random(&mut rng);

    peer_list.register_peer(test_peer);
    assert!(peer_list.is_peer_unknown(&test_peer));
    peer_list.demote_peer(test_peer);
    assert!(peer_list.is_peer_unreliable(&test_peer));
}

#[test]
fn reliable_peer_becomes_unreliable_when_demoted() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));
    let test_peer = NodeId::random(&mut rng);

    peer_list.register_peer(test_peer);
    assert!(peer_list.is_peer_unknown(&test_peer));
    peer_list.promote_peer(test_peer);
    assert!(peer_list.is_peer_reliable(&test_peer));
    peer_list.demote_peer(test_peer);
    assert!(peer_list.is_peer_unreliable(&test_peer));
}

#[test]
fn unreliable_peer_becomes_reliable_when_promoted() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));
    let test_peer = NodeId::random(&mut rng);

    peer_list.register_peer(test_peer);
    assert!(peer_list.is_peer_unknown(&test_peer));
    peer_list.demote_peer(test_peer);
    assert!(peer_list.is_peer_unreliable(&test_peer));
    peer_list.promote_peer(test_peer);
    assert!(peer_list.is_peer_reliable(&test_peer));
}

#[test]
fn unreliable_peer_remains_unreliable_if_demoted() {
    let mut rng = TestRng::new();
    let mut peer_list = PeerList::new(5, TimeDiff::from_seconds(1));
    let test_peer = NodeId::random(&mut rng);

    peer_list.register_peer(test_peer);
    assert!(peer_list.is_peer_unknown(&test_peer));
    peer_list.demote_peer(test_peer);
    assert!(peer_list.is_peer_unreliable(&test_peer));
    peer_list.demote_peer(test_peer);
    assert!(peer_list.is_peer_unreliable(&test_peer));
}
