use std::{collections::HashMap, hash::Hash, net::SocketAddr};

use serde::Serialize;

use crate::types::Block;

/// Data feed for client "info_get_status" endpoint.
#[derive(Debug, Serialize)]
#[serde(bound = "I: Eq + Hash + Serialize")]
pub struct StatusFeed<I> {
    /// The last finalized block.
    pub last_finalized_block: Option<Block>,
    /// The peer nodes which are connected to this node.
    pub peers: HashMap<I, SocketAddr>,
}

impl<I> StatusFeed<I> {
    pub(crate) fn new(last_finalized_block: Option<Block>, peers: HashMap<I, SocketAddr>) -> Self {
        StatusFeed {
            last_finalized_block,
            peers,
        }
    }
}
