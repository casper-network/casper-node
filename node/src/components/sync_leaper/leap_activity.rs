use std::{cmp::Ordering, collections::HashMap, time::Instant};

use datasize::DataSize;

use crate::types::{NodeId, SyncLeap, SyncLeapIdentifier};

use super::{leap_state::LeapState, LeapActivityError, PeerState};

#[derive(Debug, DataSize)]
pub(crate) struct LeapActivity {
    sync_leap_identifier: SyncLeapIdentifier,
    peers: HashMap<NodeId, PeerState>,
    leap_start: Instant,
}

impl LeapActivity {
    pub(crate) fn new(
        sync_leap_identifier: SyncLeapIdentifier,
        peers: HashMap<NodeId, PeerState>,
        leap_start: Instant,
    ) -> Self {
        Self {
            sync_leap_identifier,
            peers,
            leap_start,
        }
    }

    pub(super) fn status(&self) -> LeapState {
        let sync_leap_identifier = self.sync_leap_identifier;
        let in_flight = self
            .peers
            .values()
            .filter(|state| matches!(state, PeerState::RequestSent))
            .count();
        let responsed = self.peers.len() - in_flight;

        if in_flight == 0 && responsed == 0 {
            return LeapState::Failed {
                sync_leap_identifier,
                in_flight,
                error: LeapActivityError::NoPeers(sync_leap_identifier),
                from_peers: vec![],
            };
        }
        if in_flight > 0 && responsed == 0 {
            return LeapState::Awaiting {
                sync_leap_identifier,
                in_flight,
            };
        }
        match self.best_response() {
            Ok((best_available, from_peers)) => LeapState::Received {
                in_flight,
                best_available: Box::new(best_available),
                from_peers,
            },
            // `Unobtainable` means we couldn't download it from any peer so far - don't treat it
            // as a failure if there are still requests in flight
            Err(LeapActivityError::Unobtainable(_, _)) if in_flight > 0 => LeapState::Awaiting {
                sync_leap_identifier,
                in_flight,
            },
            Err(error) => LeapState::Failed {
                sync_leap_identifier,
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
        let mut maybe_ret = None;
        for (peer, peer_state) in &self.peers {
            match peer_state {
                PeerState::Fetched(sync_leap) => match &maybe_ret {
                    None => {
                        maybe_ret = Some(sync_leap);
                        peers.push(*peer);
                    }
                    Some(current_ret) => {
                        match current_ret
                            .highest_block_height()
                            .cmp(&sync_leap.highest_block_height())
                        {
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
                },
                PeerState::RequestSent | PeerState::Rejected | PeerState::CouldntFetch => {}
            }
        }

        match maybe_ret {
            Some(sync_leap) => Ok((*sync_leap.clone(), peers)),
            None => {
                if reject_count > 0 {
                    Err(LeapActivityError::TooOld(self.sync_leap_identifier, peers))
                } else {
                    Err(LeapActivityError::Unobtainable(
                        self.sync_leap_identifier,
                        peers,
                    ))
                }
            }
        }
    }

    pub(crate) fn leap_start(&self) -> Instant {
        self.leap_start
    }

    pub(crate) fn sync_leap_identifier(&self) -> &SyncLeapIdentifier {
        &self.sync_leap_identifier
    }

    pub(crate) fn peers(&self) -> &HashMap<NodeId, PeerState> {
        &self.peers
    }

    pub(crate) fn peers_mut(&mut self) -> &mut HashMap<NodeId, PeerState> {
        &mut self.peers
    }

    /// Registers new leap activity if it wasn't already registered for specified peer.
    pub(crate) fn register_peer(&mut self, peer: NodeId) -> Option<NodeId> {
        (!self.peers().contains_key(&peer)).then(|| {
            self.peers.insert(peer, PeerState::RequestSent);
            peer
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashMap},
        time::Instant,
    };

    use rand::seq::SliceRandom;

    use casper_types::{testing::TestRng, BlockHash, BlockHeader, BlockV2, TestBlockBuilder};

    use crate::{
        components::sync_leaper::{
            leap_activity::LeapActivity, tests::make_test_sync_leap, LeapActivityError, LeapState,
            PeerState,
        },
        types::{NodeId, SyncLeap, SyncLeapIdentifier},
    };

    fn make_random_block_with_height(rng: &mut TestRng, height: u64) -> BlockV2 {
        TestBlockBuilder::new()
            .era(0)
            .height(height)
            .switch_block(false)
            .build(rng)
    }

    fn make_sync_leap_with_trusted_block_header(trusted_block_header: BlockHeader) -> SyncLeap {
        SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header,
            trusted_ancestor_headers: vec![],
            signed_block_headers: vec![],
        }
    }

    fn assert_peers<I>(expected_peers: I, leap_activity: &LeapActivity)
    where
        I: IntoIterator<Item = NodeId>,
    {
        let expected_peers: BTreeSet<_> = expected_peers.into_iter().collect();
        let actual_peers: BTreeSet<_> = leap_activity
            .peers()
            .iter()
            .map(|(node_id, _)| *node_id)
            .collect();
        assert_eq!(expected_peers, actual_peers);
    }

    #[test]
    fn best_response_with_single_peer() {
        let mut rng = TestRng::new();

        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

        let sync_leap = make_test_sync_leap(&mut rng);
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
        assert_eq!(actual_peers.first().unwrap(), &peer_1.0);
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
        assert_eq!(actual_peers.first().unwrap(), &peer_1.0);
        assert_eq!(actual_sync_leap, sync_leap);
    }

    #[test]
    fn best_response_with_multiple_peers() {
        let mut rng = TestRng::new();

        // Create 10 sync leaps, each with a distinct height. The height is not greater than 10.
        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));
        let mut heights: Vec<u64> = (0..10).collect();
        heights.shuffle(&mut rng);
        let mut peers_with_sync_leaps: HashMap<_, _> = heights
            .iter()
            .map(|height| {
                let block = make_random_block_with_height(&mut rng, *height);
                let sync_leap =
                    make_sync_leap_with_trusted_block_header(block.header().clone().into());
                (
                    NodeId::random(&mut rng),
                    PeerState::Fetched(Box::new(sync_leap)),
                )
            })
            .collect();

        // Add another peer with the best response.
        let block = make_random_block_with_height(&mut rng, 500);
        let best_sync_leap =
            make_sync_leap_with_trusted_block_header(block.header().clone().into());
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
        assert_eq!(actual_peers.first().unwrap(), &peer_1_best_node_id);
        assert_eq!(actual_sync_leap, best_sync_leap);

        // Add two more peers with even better response.
        let block = make_random_block_with_height(&mut rng, 1000);
        let best_sync_leap =
            make_sync_leap_with_trusted_block_header(block.header().clone().into());
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

        // Expect two recently added peers with best sync leap to be reported.
        let mut expected_peers = vec![peer_2_best_node_id, peer_3_best_node_id];
        actual_peers.sort_unstable();
        expected_peers.sort_unstable();

        assert_eq!(actual_peers.len(), 2);
        assert_eq!(actual_peers, expected_peers);
        assert_eq!(actual_sync_leap, best_sync_leap);

        // Add two more peers with worse response.
        let block = make_random_block_with_height(&mut rng, 1);
        let worse_sync_leap =
            make_sync_leap_with_trusted_block_header(block.header().clone().into());
        let peer_3_worse_node_id = NodeId::random(&mut rng);
        let peer_4_worse_node_id = NodeId::random(&mut rng);
        leap_activity.peers.extend(
            [
                (
                    peer_3_worse_node_id,
                    PeerState::Fetched(Box::new(worse_sync_leap.clone())),
                ),
                (
                    peer_4_worse_node_id,
                    PeerState::Fetched(Box::new(worse_sync_leap)),
                ),
            ]
            .iter()
            .cloned(),
        );

        let (actual_sync_leap, mut actual_peers) = leap_activity.best_response().unwrap();

        // Expect two previously added best peers with best sync leap to be reported.
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

        let sync_leap = make_test_sync_leap(&mut rng);
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

    #[test]
    fn register_peer() {
        let mut rng = TestRng::new();

        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));

        let peer_1 = (NodeId::random(&mut rng), PeerState::RequestSent);

        let mut leap_activity = LeapActivity {
            sync_leap_identifier,
            peers: [peer_1.clone()].iter().cloned().collect(),
            leap_start: Instant::now(),
        };

        // Expect the single peer specified on creation.
        assert_peers([peer_1.0], &leap_activity);

        // Registering the same peer the second time does not register.
        let maybe_registered_peer = leap_activity.register_peer(peer_1.0);
        assert!(maybe_registered_peer.is_none());

        // Still expect only the single peer.
        assert_peers([peer_1.0], &leap_activity);

        // Registering additional peer should succeed.
        let peer_2 = NodeId::random(&mut rng);
        let maybe_registered_peer = leap_activity.register_peer(peer_2);
        assert_eq!(maybe_registered_peer, Some(peer_2));

        // But registering it for the second time should be a noop.
        let maybe_registered_peer = leap_activity.register_peer(peer_2);
        assert_eq!(maybe_registered_peer, None);

        // Expect two added peers.
        assert_peers([peer_1.0, peer_2], &leap_activity);
    }
}
