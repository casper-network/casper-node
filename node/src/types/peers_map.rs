// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::NodeId;

/// Node peer entry.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerEntry {
    /// Node id.
    pub node_id: String,
    /// Node address.
    pub address: String,
}

/// Map of peer IDs to network addresses.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeersMap(Vec<PeerEntry>);

impl PeersMap {
    /// Retrieve collection of `PeerEntry` records.
    pub fn into_inner(self) -> Vec<PeerEntry> {
        self.0
    }
}

impl From<BTreeMap<NodeId, String>> for PeersMap {
    fn from(input: BTreeMap<NodeId, String>) -> Self {
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
