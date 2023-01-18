use std::{collections::BTreeSet, sync::Arc};

use casper_types::testing::TestRng;
use prometheus::Registry;

use crate::{
    components::sync_leaper::{LeapState, PeerState, RegisterLeapAttemptOutcome},
    types::{Block, BlockHash, Chainspec, NodeId, SyncLeap, SyncLeapIdentifier},
};

use super::SyncLeaper;

pub(crate) fn make_default_sync_leap(rng: &mut TestRng) -> SyncLeap {
    let block = Block::random(rng);
    SyncLeap {
        trusted_ancestor_only: false,
        trusted_block_header: block.header().clone(),
        trusted_ancestor_headers: vec![],
        signed_block_headers: vec![],
    }
}

fn make_sync_leaper(rng: &mut TestRng) -> SyncLeaper {
    let chainspec = Chainspec::random(rng);
    let registry = Registry::new();
    SyncLeaper::new(Arc::new(chainspec), &registry).unwrap()
}

fn assert_peers(expected: &[NodeId], actual: &Vec<(NodeId, PeerState)>) {
    // Assert that all new peers are in `RequestSent` state.
    for (_, peer_state) in actual {
        assert!(matches!(peer_state, &PeerState::RequestSent));
    }

    // Assert that we have the expected list of peers.
    let expected: BTreeSet<_> = expected.iter().collect();
    let actual: BTreeSet<_> = actual.iter().map(|(node_id, _)| node_id).collect();
    assert_eq!(expected, actual);
}

#[test]
fn new_sync_leaper_has_no_activity() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);

    assert!(matches!(sync_leaper.leap_status(), LeapState::Idle));
}

#[test]
fn register_leap_attempt_no_peers() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));
    let peers_to_ask = vec![];

    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);
    assert!(matches!(outcome, RegisterLeapAttemptOutcome::DoNothing));
    assert!(sync_leaper.peers().is_none());
}

#[test]
fn register_leap_attempt_reattempt_for_different_leap_identifier() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer_1 = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer_1];

    // Start with a single peer.
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask.clone());
    // Expect that we should fetch SyncLeap from that peer.
    assert!(matches!(
        outcome,
        RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers) if peers == peers_to_ask
    ));
    let expected_peers = vec![peer_1];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);

    // Request another sync leap, but for new sync leap identifier.
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_historical(BlockHash::random(&mut rng));
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);
    // Expect that we should do nothing as the identifiers mismatch.
    assert!(matches!(outcome, RegisterLeapAttemptOutcome::DoNothing));
    let expected_peers = vec![peer_1];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);
}

#[test]
fn register_leap_attempt_with_reattempt_for_the_same_leap_identifier() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer_1 = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer_1];

    // Start with a single peer.
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask.clone());
    // Expect that we should fetch SyncLeap from that peer.
    assert!(matches!(
        outcome,
        RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers) if peers == peers_to_ask
    ));
    let expected_peers = vec![peer_1];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);

    // Try to register the same peer.
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);
    // Expect that we should do nothing as the SyncLeap from this peer has already been requested.
    assert!(matches!(outcome, RegisterLeapAttemptOutcome::DoNothing));
    let expected_peers = vec![peer_1];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);

    // Try to register one new peer.
    let peer_2 = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer_2];
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask.clone());
    // Expect that we should fetch SyncLeap from the new peer only.
    assert!(matches!(
        outcome,
        RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers) if peers == peers_to_ask
    ));
    let expected_peers = vec![peer_1, peer_2];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);

    // Try to register two already existing peers.
    let mut peers_to_ask = vec![peer_1, peer_2];
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask.clone());
    // Expect that we should do nothing as the SyncLeap from both these peers has already been
    // requested.
    assert!(matches!(outcome, RegisterLeapAttemptOutcome::DoNothing));
    let expected_peers = vec![peer_1, peer_2];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);

    // Add two new peers for a total set of four, among which two are already registered.
    let peer_3 = NodeId::random(&mut rng);
    let peer_4 = NodeId::random(&mut rng);
    peers_to_ask.push(peer_3);
    peers_to_ask.push(peer_4);
    let outcome = sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask.clone());
    // Expect that we should fetch SyncLeap from the two new peers only.
    assert!(matches!(
        outcome,
        RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers) if peers == vec![peer_3, peer_4]
    ));
    let expected_peers = vec![peer_1, peer_2, peer_3, peer_4];
    let actual_peers = sync_leaper.peers().unwrap();
    assert_peers(&expected_peers, &actual_peers);
}
