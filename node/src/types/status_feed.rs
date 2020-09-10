use std::{collections::HashMap, net::SocketAddr};

use serde::Serialize;

use crate::{small_network::NodeId, types::Block};

/// Data feed for client status endpoint.
#[derive(Debug, Serialize)]
pub struct StatusFeed {
    last_linear_block_hash: Option<String>,
    peers: Vec<String>,
}

impl StatusFeed {
    pub(crate) fn new(
        last_linear_block: Option<Block>,
        peers: HashMap<NodeId, SocketAddr>,
    ) -> Self {
        StatusFeed {
            last_linear_block_hash: last_linear_block.map(|b| hex::encode(b.hash().inner())),
            peers: peers.values().map(ToString::to_string).collect(),
        }
    }
}

impl Default for StatusFeed {
    fn default() -> Self {
        StatusFeed {
            last_linear_block_hash: None,
            peers: vec![],
        }
    }
}
