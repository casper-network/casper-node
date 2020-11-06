use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};

use crate::small_network::NodeId;

/// Map of peers.
#[derive(Serialize, Deserialize, Debug, PartialOrd, PartialEq)]
pub struct PeersMap(BTreeMap<String, SocketAddr>);

impl From<HashMap<NodeId, SocketAddr>> for PeersMap {
    fn from(input: HashMap<NodeId, SocketAddr>) -> Self {
        let ret = input
            .into_iter()
            .map(|(node_id, address)| (format!("{}", node_id), address))
            .collect();
        PeersMap(ret)
    }
}

impl From<PeersMap> for BTreeMap<String, SocketAddr> {
    fn from(input: PeersMap) -> Self {
        input.0
    }
}
