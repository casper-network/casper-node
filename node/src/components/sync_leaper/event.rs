use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::{
    components::fetcher::FetchResult,
    types::{BlockHash, NodeId, SyncLeap},
};

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    AttemptLeap {
        trusted_hash: BlockHash,
        peers_to_ask: Vec<NodeId>,
    },
    FetchedSyncLeapFromPeer {
        trusted_hash: BlockHash,
        fetch_result: FetchResult<SyncLeap>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::AttemptLeap {
                trusted_hash,
                peers_to_ask,
            } => write!(
                f,
                "sync pulling sync leap: {} {:?}",
                trusted_hash, peers_to_ask
            ),
            Event::FetchedSyncLeapFromPeer {
                trusted_hash,
                fetch_result,
            } => write!(
                f,
                "fetched sync leap from peer: {} {:?}",
                trusted_hash, fetch_result
            ),
        }
    }
}
