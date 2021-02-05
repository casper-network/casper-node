// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    collections::BTreeMap,
    hash::Hash,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use once_cell::sync::Lazy;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_types::PublicKey;

use crate::{
    components::{
        chainspec_loader::NextUpgrade,
        consensus::EraId,
        rpc_server::rpcs::docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    },
    crypto::hash::Digest,
    types::{ActivationPoint, Block, BlockHash, NodeId, PeersMap, Timestamp},
};

static CHAINSPEC_INFO: Lazy<ChainspecInfo> = Lazy::new(|| {
    let next_upgrade =
        NextUpgrade::new(ActivationPoint { era_id: EraId(42) }, Version::new(2, 0, 1));
    ChainspecInfo {
        name: String::from("casper-example"),
        state_root_hash: Some(Digest::from([2u8; Digest::LENGTH])),
        next_upgrade: Some(next_upgrade),
    }
});

static GET_STATUS_RESULT: Lazy<GetStatusResult> = Lazy::new(|| {
    let node_id = NodeId::doc_example();
    let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 54321);
    let mut peers = BTreeMap::new();
    peers.insert(node_id.clone(), socket_addr.to_string());
    let status_feed = StatusFeed::<NodeId> {
        last_added_block: Some(Block::doc_example().clone()),
        peers,
        chainspec_info: ChainspecInfo::doc_example().clone(),
        version: crate::VERSION_STRING.as_str(),
    };
    GetStatusResult::new(status_feed, DOCS_EXAMPLE_PROTOCOL_VERSION.clone())
});

/// Summary information from the chainspec.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChainspecInfo {
    /// Name of the network.
    name: String,
    /// If `Some` then genesis or upgrade process returned a valid post state hash.
    state_root_hash: Option<Digest>,
    next_upgrade: Option<NextUpgrade>,
}

impl DocExample for ChainspecInfo {
    fn doc_example() -> &'static Self {
        &*CHAINSPEC_INFO
    }
}

impl ChainspecInfo {
    pub(crate) fn new(
        chainspec_network_name: String,
        state_root_hash: Option<Digest>,
        next_upgrade: Option<NextUpgrade>,
    ) -> Self {
        ChainspecInfo {
            name: chainspec_network_name,
            state_root_hash,
            next_upgrade,
        }
    }
}

/// Data feed for client "info_get_status" endpoint.
#[derive(Debug, Serialize)]
#[serde(bound = "I: Eq + Hash + Ord + Serialize")]
pub struct StatusFeed<I> {
    /// The last block added to the chain.
    pub last_added_block: Option<Block>,
    /// The peer nodes which are connected to this node.
    pub peers: BTreeMap<I, String>,
    /// The chainspec info for this node.
    pub chainspec_info: ChainspecInfo,
    /// The compiled node version.
    pub version: &'static str,
}

impl<I> StatusFeed<I> {
    pub(crate) fn new(
        last_added_block: Option<Block>,
        peers: BTreeMap<I, String>,
        chainspec_info: ChainspecInfo,
    ) -> Self {
        StatusFeed {
            last_added_block,
            peers,
            chainspec_info,
            version: crate::VERSION_STRING.as_str(),
        }
    }
}

/// Minimal info of a `Block`.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MinimalBlockInfo {
    hash: BlockHash,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    state_root_hash: Digest,
    creator: PublicKey,
}

impl From<Block> for MinimalBlockInfo {
    fn from(block: Block) -> Self {
        MinimalBlockInfo {
            hash: *block.hash(),
            timestamp: block.header().timestamp(),
            era_id: block.header().era_id(),
            height: block.header().height(),
            state_root_hash: *block.header().state_root_hash(),
            creator: *block.body().proposer(),
        }
    }
}

/// Result for "info_get_status" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: Version,
    /// The chainspec name.
    pub chainspec_name: String,
    /// The chainspec state root hash.
    pub chainspec_state_root_hash: String,
    /// The node ID and network address of each connected peer.
    pub peers: PeersMap,
    /// The minimal info of the last block from the linear chain.
    pub last_added_block_info: Option<MinimalBlockInfo>,
    /// Information about the next scheduled upgrade.
    pub next_upgrade: Option<NextUpgrade>,
    /// The compiled node version.
    pub build_version: String,
}

impl GetStatusResult {
    pub(crate) fn new(status_feed: StatusFeed<NodeId>, api_version: Version) -> Self {
        let chainspec_name = status_feed.chainspec_info.name;
        let chainspec_state_root_hash = status_feed
            .chainspec_info
            .state_root_hash
            .unwrap_or_default()
            .to_string();
        let peers = PeersMap::from(status_feed.peers);
        let last_added_block_info = status_feed.last_added_block.map(Into::into);
        let next_upgrade = status_feed.chainspec_info.next_upgrade;
        let build_version = crate::VERSION_STRING.clone();
        GetStatusResult {
            api_version,
            chainspec_name,
            chainspec_state_root_hash,
            peers,
            last_added_block_info,
            next_upgrade,
            build_version,
        }
    }
}

impl DocExample for GetStatusResult {
    fn doc_example() -> &'static Self {
        &*GET_STATUS_RESULT
    }
}
