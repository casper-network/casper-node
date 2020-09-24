use std::{collections::HashMap, hash::Hash, net::SocketAddr};

use serde::Serialize;

use crate::{components::chainspec_loader::ChainspecInfo, types::Block};

/// Data feed for client "info_get_status" endpoint.
#[derive(Debug, Serialize)]
#[serde(bound = "I: Eq + Hash + Serialize")]
pub struct StatusFeed<I> {
    /// The last finalized block.
    pub last_finalized_block: Option<Block>,
    /// The peer nodes which are connected to this node.
    pub peers: HashMap<I, SocketAddr>,
    /// The chainspec info for this node.
    pub chainspec_info: ChainspecInfo,
}

impl<I> StatusFeed<I> {
    pub(crate) fn new(
        last_finalized_block: Option<Block>,
        peers: HashMap<I, SocketAddr>,
        chainspec_info: ChainspecInfo,
    ) -> Self {
        StatusFeed {
            last_finalized_block,
            peers,
            chainspec_info,
        }
    }
}
