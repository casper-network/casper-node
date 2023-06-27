use std::fmt::{Display, Formatter};

use datasize::DataSize;

use crate::types::{NodeId, SyncLeap, SyncLeapIdentifier};

use super::LeapActivityError;

#[derive(Debug, DataSize)]
pub(crate) enum LeapState {
    Idle,
    Awaiting {
        sync_leap_identifier: SyncLeapIdentifier,
        in_flight: usize,
    },
    Received {
        best_available: Box<SyncLeap>,
        from_peers: Vec<NodeId>,
        in_flight: usize,
    },
    Failed {
        sync_leap_identifier: SyncLeapIdentifier,
        error: LeapActivityError,
        from_peers: Vec<NodeId>,
        in_flight: usize,
    },
}

impl Display for LeapState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LeapState::Idle => {
                write!(f, "Idle")
            }
            LeapState::Awaiting {
                sync_leap_identifier,
                in_flight,
            } => {
                write!(
                    f,
                    "Awaiting {} responses for {}",
                    in_flight,
                    sync_leap_identifier.block_hash(),
                )
            }
            LeapState::Received {
                best_available,
                from_peers,
                in_flight,
            } => {
                write!(
                    f,
                    "Received {} from {} peers, awaiting {} responses",
                    best_available.highest_block_hash(),
                    from_peers.len(),
                    in_flight
                )
            }
            LeapState::Failed {
                sync_leap_identifier,
                error,
                ..
            } => {
                write!(
                    f,
                    "Failed leap for {} {}",
                    sync_leap_identifier.block_hash(),
                    error
                )
            }
        }
    }
}

impl LeapState {
    pub(super) fn in_flight(&self) -> usize {
        match self {
            LeapState::Idle => 0,
            LeapState::Awaiting { in_flight, .. }
            | LeapState::Received { in_flight, .. }
            | LeapState::Failed { in_flight, .. } => *in_flight,
        }
    }

    pub(super) fn active(&self) -> bool {
        self.in_flight() > 0
    }
}

#[cfg(test)]
mod tests {
    use casper_types::{testing::TestRng, BlockHash};

    use crate::{
        components::sync_leaper::{tests::make_test_sync_leap, LeapActivityError, LeapState},
        types::SyncLeapIdentifier,
    };

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

        let sync_leap = make_test_sync_leap(&mut rng);
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
}
