use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::{
    components::fetcher::FetchResult,
    types::{NodeId, SyncLeap, SyncLeapIdentifier},
};

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    AttemptLeap {
        sync_leap_identifier: SyncLeapIdentifier,
        peers_to_ask: Vec<NodeId>,
    },
    FetchedSyncLeapFromPeer {
        sync_leap_identifier: SyncLeapIdentifier,
        fetch_result: FetchResult<SyncLeap>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            } => write!(
                f,
                "sync pulling sync leap: {:?} {:?}",
                sync_leap_identifier, peers_to_ask
            ),
            Event::FetchedSyncLeapFromPeer {
                sync_leap_identifier,
                fetch_result,
            } => write!(
                f,
                "fetched sync leap from peer: {} {:?}",
                sync_leap_identifier, fetch_result
            ),
        }
    }
}
