use std::{collections::HashMap, net::SocketAddr};

use serde::{Deserialize, Serialize};

use crate::types::NodeId;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
struct PeerEntry {
    node_id: String,
    address: SocketAddr,
}

/// Map of peer IDs to network addresses.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PeersMap(Vec<PeerEntry>);

impl From<HashMap<NodeId, SocketAddr>> for PeersMap {
    fn from(input: HashMap<NodeId, SocketAddr>) -> Self {
        let ret = input
            .into_iter()
            .map(|(node_id, address)| PeerEntry {
                node_id: format!("{}", node_id),
                address,
            })
            .collect();
        PeersMap(ret)
    }
}
