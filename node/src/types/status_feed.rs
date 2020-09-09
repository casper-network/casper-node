use serde::Serialize;

use crate::types::Block;

/// Data feed for client status endpoint.
#[derive(Debug, Serialize)]
pub struct StatusFeed {
    last_finalized_block: Option<Block>,
    //peers: HashMap<NodeId, SocketAddr>,
}

impl StatusFeed {
    pub(crate) fn new(
        last_finalized_block: Option<Block>,
        //peers: HashMap<NodeId, SocketAddr>,
    ) -> Self {
        StatusFeed {
            last_finalized_block,
            //peers,
        }
    }
}
