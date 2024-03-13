// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{collections::BTreeMap, net::SocketAddr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::NodeId;

/// Node peer entry.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerEntry {
    /// Node id.
    pub node_id: String,
    /// Node address.
    pub address: SocketAddr,
}

/// Map of peer IDs to network addresses.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeersMap(Vec<PeerEntry>);

impl PeersMap {
    /// Retrieve collection of `PeerEntry` records.
    pub fn into_inner(self) -> Vec<PeerEntry> {
        self.0
    }
}

impl From<BTreeMap<NodeId, SocketAddr>> for PeersMap {
    fn from(input: BTreeMap<NodeId, SocketAddr>) -> Self {
        let ret = input
            .into_iter()
            .map(|(node_id, address)| PeerEntry {
                node_id: node_id.to_string(),
                address,
            })
            .collect();
        PeersMap(ret)
    }
}
