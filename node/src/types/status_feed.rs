use std::{collections::HashMap, net::SocketAddr};

use serde::Serialize;

use crate::{components::consensus::EraId, small_network::NodeId, types::Block};

use super::Timestamp;

#[derive(Debug, Serialize)]
struct LfbData {
    hash: String,
    height: u64,
    era_id: EraId,
    timestamp: Timestamp,
}

impl From<Block> for LfbData {
    fn from(b: Block) -> Self {
        LfbData {
            hash: hex::encode(b.hash().inner()),
            height: b.height(),
            era_id: b.era_id(),
            timestamp: b.timestamp(),
        }
    }
}

/// Data feed for client status endpoint.
#[derive(Debug, Serialize)]
pub struct StatusFeed {
    last_linear_block: Option<LfbData>,
    peers: Vec<String>,
}

impl StatusFeed {
    pub(crate) fn new(
        last_linear_block: Option<Block>,
        peers: HashMap<NodeId, SocketAddr>,
    ) -> Self {
        StatusFeed {
            last_linear_block: last_linear_block.map(|b| b.into()),
            peers: peers.values().map(ToString::to_string).collect(),
        }
    }
}

impl Default for StatusFeed {
    fn default() -> Self {
        StatusFeed {
            last_linear_block: None,
            peers: vec![],
        }
    }
}
