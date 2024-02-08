use std::{collections::BTreeSet, sync::Arc};

use prometheus::Registry;

use casper_types::{testing::TestRng, BlockHash, Chainspec, TestBlockBuilder};

use crate::{
    components::{
        fetcher::{self, FetchResult, FetchedData},
        sync_leaper::{LeapState, PeerState, RegisterLeapAttemptOutcome},
    },
    types::{NodeId, SyncLeap, SyncLeapIdentifier},
};

use super::{Error, SyncLeaper};

pub(crate) fn make_test_sync_leap(rng: &mut TestRng) -> SyncLeap {
    let block = TestBlockBuilder::new().build_versioned(rng);
    SyncLeap {
        trusted_ancestor_only: false,
        trusted_block_header: block.clone_header(),
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

fn assert_peer(sync_leaper: SyncLeaper, (expected_peer, expected_peer_state): (NodeId, PeerState)) {
    let peers = sync_leaper.peers().unwrap();
    let (node_id, actual_peer_state) = peers.first().unwrap();
    assert_eq!(node_id, &expected_peer);
    assert_eq!(actual_peer_state, &expected_peer_state);
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

#[test]
fn fetch_received_from_storage() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap = make_test_sync_leap(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer_1 = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer_1];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let fetch_result: FetchResult<SyncLeap> = Ok(FetchedData::from_storage(Box::new(sync_leap)));

    let actual = sync_leaper
        .fetch_received(sync_leap_identifier, fetch_result)
        .unwrap_err();
    assert!(matches!(actual, Error::FetchedSyncLeapFromStorage(_)));
}

#[test]
fn fetch_received_identifier_mismatch() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap = make_test_sync_leap(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let fetch_result: FetchResult<SyncLeap> = Ok(FetchedData::from_peer(sync_leap, peer));

    let different_sync_leap_identifier =
        SyncLeapIdentifier::sync_to_historical(BlockHash::random(&mut rng));

    let actual = sync_leaper
        .fetch_received(different_sync_leap_identifier, fetch_result)
        .unwrap_err();

    assert!(matches!(actual, Error::SyncLeapIdentifierMismatch { .. }));
}

#[test]
fn fetch_received_unexpected_response() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap = make_test_sync_leap(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let fetch_result: FetchResult<SyncLeap> = Ok(FetchedData::from_peer(sync_leap, peer));

    let actual = sync_leaper
        .fetch_received(sync_leap_identifier, fetch_result)
        .unwrap_err();
    assert!(matches!(actual, Error::UnexpectedSyncLeapResponse(_)));

    let peers = sync_leaper.peers();
    assert!(peers.is_none());
}

#[test]
fn fetch_received_from_unknown_peer() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap = make_test_sync_leap(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let unknown_peer = NodeId::random(&mut rng);
    let fetch_result: FetchResult<SyncLeap> = Ok(FetchedData::from_peer(sync_leap, unknown_peer));

    let actual = sync_leaper
        .fetch_received(sync_leap_identifier, fetch_result)
        .unwrap_err();
    assert!(matches!(actual, Error::ResponseFromUnknownPeer { .. }));

    assert_peer(sync_leaper, (peer, PeerState::RequestSent));
}

#[test]
fn fetch_received_correctly() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap = make_test_sync_leap(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let fetch_result: FetchResult<SyncLeap> = Ok(FetchedData::from_peer(sync_leap.clone(), peer));

    let actual = sync_leaper.fetch_received(sync_leap_identifier, fetch_result);
    assert!(actual.is_ok());

    assert_peer(sync_leaper, (peer, PeerState::Fetched(Box::new(sync_leap))));
}

#[test]
fn fetch_received_peer_rejected() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let fetch_result: FetchResult<SyncLeap> = Err(fetcher::Error::Rejected {
        id: Box::new(sync_leap_identifier),
        peer,
    });

    let actual = sync_leaper.fetch_received(sync_leap_identifier, fetch_result);
    assert!(actual.is_ok());

    assert_peer(sync_leaper, (peer, PeerState::Rejected));
}

#[test]
fn fetch_received_from_unknown_peer_rejected() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let unknown_peer = NodeId::random(&mut rng);
    let fetch_result: FetchResult<SyncLeap> = Err(fetcher::Error::Rejected {
        id: Box::new(sync_leap_identifier),
        peer: unknown_peer,
    });

    let actual = sync_leaper
        .fetch_received(sync_leap_identifier, fetch_result)
        .unwrap_err();
    assert!(matches!(actual, Error::ResponseFromUnknownPeer { .. }));

    assert_peer(sync_leaper, (peer, PeerState::RequestSent));
}

#[test]
fn fetch_received_other_error() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let fetch_result: FetchResult<SyncLeap> = Err(fetcher::Error::TimedOut {
        id: Box::new(sync_leap_identifier),
        peer,
    });

    let actual = sync_leaper.fetch_received(sync_leap_identifier, fetch_result);
    assert!(actual.is_ok());

    assert_peer(sync_leaper, (peer, PeerState::CouldntFetch));
}

#[test]
fn fetch_received_from_unknown_peer_other_error() {
    let mut rng = TestRng::new();

    let mut sync_leaper = make_sync_leaper(&mut rng);
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer = NodeId::random(&mut rng);
    let peers_to_ask = vec![peer];
    sync_leaper.register_leap_attempt(sync_leap_identifier, peers_to_ask);

    let unknown_peer = NodeId::random(&mut rng);
    let fetch_result: FetchResult<SyncLeap> = Err(fetcher::Error::TimedOut {
        id: Box::new(sync_leap_identifier),
        peer: unknown_peer,
    });

    let actual = sync_leaper
        .fetch_received(sync_leap_identifier, fetch_result)
        .unwrap_err();
    assert!(matches!(actual, Error::ResponseFromUnknownPeer { .. }));

    assert_peer(sync_leaper, (peer, PeerState::RequestSent));
}
