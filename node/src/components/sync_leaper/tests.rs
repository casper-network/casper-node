use std::{collections::HashMap, time::Instant};

use casper_types::{testing::TestRng, ProtocolVersion};
use rand::seq::SliceRandom;

use crate::{
    components::sync_leaper::LeapActivityError,
    types::{Block, BlockHash, BlockHeader, NodeId, SyncLeap, SyncLeapIdentifier},
};

use super::{LeapActivity, LeapState, PeerState};

fn make_random_block_with_height(rng: &mut TestRng, height: u64) -> Block {
    Block::random_with_specifics(
        rng,
        0.into(),
        height,
        ProtocolVersion::default(),
        false,
        None,
    )
}

fn make_default_sync_leap(rng: &mut TestRng) -> SyncLeap {
    let block = Block::random(rng);
    SyncLeap {
        trusted_ancestor_only: false,
        trusted_block_header: block.header().clone(),
        trusted_ancestor_headers: vec![],
        signed_block_headers: vec![],
    }
}

fn make_sync_leap_with_trusted_block_header(trusted_block_header: BlockHeader) -> SyncLeap {
    SyncLeap {
        trusted_ancestor_only: false,
        trusted_block_header,
        trusted_ancestor_headers: vec![],
        signed_block_headers: vec![],
    }
}

#[test]
fn leap_state() {
    let mut rng = TestRng::new();

    let leap_state = LeapState::Idle;
    assert!(!leap_state.active());

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));
    let leap_state = LeapState::Awaiting {
        sync_leap_identifier,
        in_flight: 0,
    };
    assert!(!leap_state.active());
    assert_eq!(leap_state.in_flight(), 0);

    let leap_state = LeapState::Awaiting {
        sync_leap_identifier,
        in_flight: 1,
    };
    assert!(leap_state.active());
    assert_eq!(leap_state.in_flight(), 1);

    let leap_state = LeapState::Failed {
        sync_leap_identifier,
        in_flight: 0,
        error: LeapActivityError::NoPeers(sync_leap_identifier),
        from_peers: vec![],
    };
    assert!(!leap_state.active());
    assert_eq!(leap_state.in_flight(), 0);

    let leap_state = LeapState::Failed {
        sync_leap_identifier,
        in_flight: 1,
        error: LeapActivityError::NoPeers(sync_leap_identifier),
        from_peers: vec![],
    };
    assert!(leap_state.active());
    assert_eq!(leap_state.in_flight(), 1);

    let sync_leap = make_default_sync_leap(&mut rng);
    let leap_state = LeapState::Received {
        best_available: Box::new(sync_leap.clone()),
        from_peers: vec![],
        in_flight: 0,
    };
    assert!(!leap_state.active());
    assert_eq!(leap_state.in_flight(), 0);

    let leap_state = LeapState::Received {
        best_available: Box::new(sync_leap),
        from_peers: vec![],
        in_flight: 1,
    };
    assert!(leap_state.active());
    assert_eq!(leap_state.in_flight(), 1);
}

#[test]
fn best_response_with_single_peer() {
    let mut rng = TestRng::new();

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let sync_leap = make_default_sync_leap(&mut rng);
    let peer_1 = (
        NodeId::random(&mut rng),
        PeerState::Fetched(Box::new(sync_leap.clone())),
    );

    let mut leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: [peer_1.clone()].iter().cloned().collect(),
        leap_start: Instant::now(),
    };

    let (actual_sync_leap, actual_peers) = leap_activity.best_response().unwrap();

    assert!(!actual_peers.is_empty());
    assert_eq!(actual_peers.get(0).unwrap(), &peer_1.0);
    assert_eq!(actual_sync_leap, sync_leap);

    // Adding peers in other states does not change the result.
    let peer_request_sent = (NodeId::random(&mut rng), PeerState::RequestSent);
    let peer_couldnt_fetch = (NodeId::random(&mut rng), PeerState::CouldntFetch);
    let peer_rejected = (NodeId::random(&mut rng), PeerState::Rejected);
    leap_activity.peers.extend(
        [peer_request_sent, peer_couldnt_fetch, peer_rejected]
            .iter()
            .cloned(),
    );

    let (actual_sync_leap, actual_peers) = leap_activity.best_response().unwrap();

    assert_eq!(actual_peers.len(), 1);
    assert_eq!(actual_peers.get(0).unwrap(), &peer_1.0);
    assert_eq!(actual_sync_leap, sync_leap);
}

#[test]
fn best_response_with_multiple_peers() {
    let mut rng = TestRng::new();

    // Create 10 sync leaps, each with a distinct height. The height is not greater than 10
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));
    let mut heights: Vec<u64> = (0..10).collect();
    heights.shuffle(&mut rng);
    let mut peers_with_sync_leaps: HashMap<_, _> = heights
        .iter()
        .map(|height| {
            let block = make_random_block_with_height(&mut rng, *height);
            let sync_leap = make_sync_leap_with_trusted_block_header(block.header().clone());
            (
                NodeId::random(&mut rng),
                PeerState::Fetched(Box::new(sync_leap)),
            )
        })
        .collect();

    // Add another peer with the best response.
    let block = make_random_block_with_height(&mut rng, 500);
    let best_sync_leap = make_sync_leap_with_trusted_block_header(block.header().clone());
    let peer_1_best_node_id = NodeId::random(&mut rng);
    peers_with_sync_leaps.insert(
        peer_1_best_node_id,
        PeerState::Fetched(Box::new(best_sync_leap.clone())),
    );
    let mut leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: peers_with_sync_leaps.clone(),
        leap_start: Instant::now(),
    };

    let (actual_sync_leap, actual_peers) = leap_activity.best_response().unwrap();

    // Expect only a single peer with the best sync leap.
    assert_eq!(actual_peers.len(), 1);
    assert_eq!(actual_peers.get(0).unwrap(), &peer_1_best_node_id);
    assert_eq!(actual_sync_leap, best_sync_leap);

    // Add two more peers with even better response.
    let block = make_random_block_with_height(&mut rng, 1000);
    let best_sync_leap = make_sync_leap_with_trusted_block_header(block.header().clone());
    let peer_2_best_node_id = NodeId::random(&mut rng);
    let peer_3_best_node_id = NodeId::random(&mut rng);
    leap_activity.peers.extend(
        [
            (
                peer_2_best_node_id,
                PeerState::Fetched(Box::new(best_sync_leap.clone())),
            ),
            (
                peer_3_best_node_id,
                PeerState::Fetched(Box::new(best_sync_leap.clone())),
            ),
        ]
        .iter()
        .cloned(),
    );

    let (actual_sync_leap, mut actual_peers) = leap_activity.best_response().unwrap();

    // Expect two recently added peers with best sync leap to be reported..
    let mut expected_peers = vec![peer_2_best_node_id, peer_3_best_node_id];
    actual_peers.sort_unstable();
    expected_peers.sort_unstable();

    assert_eq!(actual_peers.len(), 2);
    assert_eq!(actual_peers, expected_peers);
    assert_eq!(actual_sync_leap, best_sync_leap);

    // Adding peers in other states does not change the result.
    let peer_request_sent = (NodeId::random(&mut rng), PeerState::RequestSent);
    let peer_couldnt_fetch = (NodeId::random(&mut rng), PeerState::CouldntFetch);
    let peer_rejected = (NodeId::random(&mut rng), PeerState::Rejected);
    leap_activity.peers.extend(
        [peer_request_sent, peer_couldnt_fetch, peer_rejected]
            .iter()
            .cloned(),
    );

    let (actual_sync_leap, mut actual_peers) = leap_activity.best_response().unwrap();
    let mut expected_peers = vec![peer_2_best_node_id, peer_3_best_node_id];
    actual_peers.sort_unstable();
    expected_peers.sort_unstable();
    assert_eq!(actual_peers.len(), 2);
    assert_eq!(actual_peers, expected_peers);
    assert_eq!(actual_sync_leap, best_sync_leap);
}

#[test]
fn best_response_failed() {
    let mut rng = TestRng::new();

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer_couldnt_fetch = (NodeId::random(&mut rng), PeerState::CouldntFetch);
    let peer_request_sent = (NodeId::random(&mut rng), PeerState::RequestSent);

    let mut leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: [peer_couldnt_fetch, peer_request_sent]
            .iter()
            .cloned()
            .collect(),
        leap_start: Instant::now(),
    };

    let best_response_error = leap_activity.best_response().unwrap_err();
    assert!(matches!(
        best_response_error,
        LeapActivityError::Unobtainable(_, _)
    ));

    leap_activity
        .peers
        .insert(NodeId::random(&mut rng), PeerState::Rejected);
    let best_response_error = leap_activity.best_response().unwrap_err();
    assert!(matches!(
        best_response_error,
        LeapActivityError::TooOld(_, _)
    ));
}

#[test]
fn leap_activity_status_failed() {
    let mut rng = TestRng::new();

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: HashMap::new(),
        leap_start: Instant::now(),
    };
    assert!(matches!(
        leap_activity.status(),
        LeapState::Failed { error, .. } if matches!(error, LeapActivityError::NoPeers(_))
    ));

    let peer_1 = (NodeId::random(&mut rng), PeerState::CouldntFetch);
    let leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: [peer_1].iter().cloned().collect(),
        leap_start: Instant::now(),
    };
    assert!(matches!(
        leap_activity.status(),
        LeapState::Failed { error, .. } if matches!(error, LeapActivityError::Unobtainable(_, _))
    ));
}

#[test]
fn leap_activity_status_awaiting() {
    let mut rng = TestRng::new();

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let peer_1 = (NodeId::random(&mut rng), PeerState::RequestSent);
    let peer_2 = (NodeId::random(&mut rng), PeerState::RequestSent);
    let mut leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: [peer_1, peer_2].iter().cloned().collect(),
        leap_start: Instant::now(),
    };
    assert!(matches!(leap_activity.status(), LeapState::Awaiting { .. }));

    leap_activity
        .peers
        .insert(NodeId::random(&mut rng), PeerState::CouldntFetch);
    assert!(matches!(leap_activity.status(), LeapState::Awaiting { .. }));
}

#[test]
fn leap_activity_status_received() {
    let mut rng = TestRng::new();

    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

    let sync_leap = make_default_sync_leap(&mut rng);
    let peer_1 = (
        NodeId::random(&mut rng),
        PeerState::Fetched(Box::new(sync_leap)),
    );
    let mut leap_activity = LeapActivity {
        sync_leap_identifier,
        peers: [peer_1].iter().cloned().collect(),
        leap_start: Instant::now(),
    };
    assert!(matches!(leap_activity.status(), LeapState::Received { .. }));

    // Adding peers in other states does not change the result.
    let peer_request_sent = (NodeId::random(&mut rng), PeerState::RequestSent);
    let peer_couldnt_fetch = (NodeId::random(&mut rng), PeerState::CouldntFetch);
    let peer_rejected = (NodeId::random(&mut rng), PeerState::Rejected);
    leap_activity.peers.extend(
        [peer_request_sent, peer_couldnt_fetch, peer_rejected]
            .iter()
            .cloned(),
    );

    assert!(matches!(leap_activity.status(), LeapState::Received { .. }));
}
