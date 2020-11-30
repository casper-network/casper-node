// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{collections::HashMap, net::SocketAddr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::NodeId;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PeerEntry {
    node_id: String,
    address: SocketAddr,
}

/// Map of peer IDs to network addresses.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
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
