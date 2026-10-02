use std::{collections::BTreeMap, sync::Arc, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_binary_port::ConsensusStatus;
use casper_types::{
    AvailableBlockRange, Block, BlockHash, BlockSynchronizerStatus, ChainspecRawBytes, Digest,
    EraId, NextUpgrade, Peers, ProtocolVersion, PublicKey, TimeDiff, Timestamp,
};

use crate::{reactor::main_reactor::ReactorState, types::NodeId};

/// Complete source documents for the running and installed future chainspecs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chainspecs {
    /// Source documents loaded by the running node, including optional companion files.
    pub current: Arc<ChainspecRawBytes>,
    /// Installed future source documents, keyed and ordered by protocol version.
    pub future: BTreeMap<ProtocolVersion, Arc<ChainspecRawBytes>>,
}

/// Chainspec discovery availability, independent of status and upgrade selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ChainspecStatus {
    /// Complete, atomically refreshed chainspec source documents.
    Available(Chainspecs),
    /// Installed chainspec source documents could not be read or identified.
    Unavailable {
        /// Diagnostic for the invalid installed chainspec.
        error: String,
    },
}

impl From<Result<Chainspecs, String>> for ChainspecStatus {
    fn from(result: Result<Chainspecs, String>) -> Self {
        match result {
            Ok(chainspecs) => Self::Available(chainspecs),
            Err(error) => Self::Unavailable { error },
        }
    }
}

/// Summary information from the chainspec.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChainspecInfo {
    /// Name of the network.
    name: String,
    next_upgrade: Option<NextUpgrade>,
    chainspecs: ChainspecStatus,
}

impl ChainspecInfo {
    pub(crate) fn new(
        chainspec_network_name: String,
        next_upgrade: Option<NextUpgrade>,
        chainspecs: ChainspecStatus,
    ) -> Self {
        ChainspecInfo {
            name: chainspec_network_name,
            next_upgrade,
            chainspecs,
        }
    }
}

/// Data feed for client "info_get_status" endpoint.
#[derive(Debug, Serialize)]
pub struct StatusFeed {
    /// The last block added to the chain.
    pub last_added_block: Option<Block>,
    /// The peer nodes which are connected to this node.
    pub peers: BTreeMap<NodeId, String>,
    /// The chainspec info for this node.
    pub chainspec_info: ChainspecInfo,
    /// Our public signing key.
    pub our_public_signing_key: Option<PublicKey>,
    /// The next round length if this node is a validator.
    pub round_length: Option<TimeDiff>,
    /// The compiled node version.
    pub version: &'static str,
    /// Time that passed since the node has started.
    pub node_uptime: Duration,
    /// The current state of node reactor.
    pub reactor_state: ReactorState,
    /// Timestamp of the last recorded progress in the reactor.
    pub last_progress: Timestamp,
    /// The available block range in storage.
    pub available_block_range: AvailableBlockRange,
    /// The status of the block synchronizer builders.
    pub block_sync: BlockSynchronizerStatus,
    /// The state root hash of the lowest block in the available block range.
    pub starting_state_root_hash: Digest,
    /// The hash of the latest switch block.
    pub latest_switch_block_hash: Option<BlockHash>,
}

impl StatusFeed {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        last_added_block: Option<Block>,
        peers: BTreeMap<NodeId, String>,
        chainspec_info: ChainspecInfo,
        consensus_status: Option<ConsensusStatus>,
        node_uptime: Duration,
        reactor_state: ReactorState,
        last_progress: Timestamp,
        available_block_range: AvailableBlockRange,
        block_sync: BlockSynchronizerStatus,
        starting_state_root_hash: Digest,
        latest_switch_block_hash: Option<BlockHash>,
    ) -> Self {
        let (our_public_signing_key, round_length) =
            consensus_status.map_or((None, None), |consensus_status| {
                (
                    Some(consensus_status.validator_public_key().clone()),
                    consensus_status.round_length(),
                )
            });
        StatusFeed {
            last_added_block,
            peers,
            chainspec_info,
            our_public_signing_key,
            round_length,
            version: crate::VERSION_STRING.as_str(),
            node_uptime,
            reactor_state,
            last_progress,
            available_block_range,
            block_sync,
            starting_state_root_hash,
            latest_switch_block_hash,
        }
    }
}

/// Minimal info of a `Block`.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
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
        let proposer = match &block {
            Block::V1(v1) => v1.proposer().clone(),
            Block::V2(v2) => v2.proposer().clone(),
        };

        MinimalBlockInfo {
            hash: *block.hash(),
            timestamp: block.timestamp(),
            era_id: block.era_id(),
            height: block.height(),
            state_root_hash: *block.state_root_hash(),
            creator: proposer,
        }
    }
}

/// Result for "info_get_status" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusResult {
    /// The node ID and network address of each connected peer.
    pub peers: Peers,
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The compiled node version.
    pub build_version: String,
    /// The chainspec name.
    pub chainspec_name: String,
    /// The state root hash of the lowest block in the available block range.
    pub starting_state_root_hash: Digest,
    /// The minimal info of the last block from the linear chain.
    pub last_added_block_info: Option<MinimalBlockInfo>,
    /// Our public signing key.
    pub our_public_signing_key: Option<PublicKey>,
    /// The next round length if this node is a validator.
    pub round_length: Option<TimeDiff>,
    /// Information about the next scheduled upgrade.
    pub next_upgrade: Option<NextUpgrade>,
    /// Complete running and installed future chainspec source documents, or an error diagnostic.
    pub chainspecs: ChainspecStatus,
    /// Time that passed since the node has started.
    pub uptime: TimeDiff,
    /// The current state of node reactor.
    pub reactor_state: ReactorState,
    /// Timestamp of the last recorded progress in the reactor.
    pub last_progress: Timestamp,
    /// The available block range in storage.
    pub available_block_range: AvailableBlockRange,
    /// The status of the block synchronizer builders.
    pub block_sync: BlockSynchronizerStatus,
    /// The hash of the latest switch block.
    pub latest_switch_block_hash: Option<BlockHash>,
}

impl GetStatusResult {
    #[allow(deprecated)]
    pub(crate) fn new(status_feed: StatusFeed, api_version: ProtocolVersion) -> Self {
        GetStatusResult {
            peers: Peers::from(status_feed.peers),
            api_version,
            chainspec_name: status_feed.chainspec_info.name,
            starting_state_root_hash: status_feed.starting_state_root_hash,
            last_added_block_info: status_feed.last_added_block.map(Into::into),
            our_public_signing_key: status_feed.our_public_signing_key,
            round_length: status_feed.round_length,
            next_upgrade: status_feed.chainspec_info.next_upgrade,
            chainspecs: status_feed.chainspec_info.chainspecs,
            uptime: status_feed.node_uptime.into(),
            reactor_state: status_feed.reactor_state,
            last_progress: status_feed.last_progress,
            available_block_range: status_feed.available_block_range,
            block_sync: status_feed.block_sync,
            latest_switch_block_hash: status_feed.latest_switch_block_hash,
            #[cfg(not(test))]
            build_version: crate::VERSION_STRING.clone(),

            //  Prevent these values from changing between test sessions
            #[cfg(test)]
            build_version: String::from("1.0.0-xxxxxxxxx@DEBUG"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(bytes: &[u8]) -> Arc<ChainspecRawBytes> {
        Arc::new(ChainspecRawBytes::new(bytes.to_vec().into(), None, None))
    }

    #[test]
    fn chainspec_status_roundtrips_complete_source_documents() {
        let current = Arc::new(ChainspecRawBytes::new(
            b"# complete chainspec source\n".to_vec().into(),
            Some(b"# accounts\n".to_vec().into()),
            Some(b"# state\n".to_vec().into()),
        ));
        let status = ChainspecStatus::Available(Chainspecs {
            current,
            future: BTreeMap::from([
                (ProtocolVersion::from_parts(10, 0, 0), source(b"# last\n")),
                (ProtocolVersion::from_parts(2, 2, 0), source(b"# next\n")),
            ]),
        });
        let json = serde_json::to_value(&status).unwrap();
        assert!(json["current"]["chainspec_bytes"].is_string());
        assert!(json["future"]["2.2.0"]["chainspec_bytes"].is_string());
        assert_eq!(
            serde_json::from_value::<ChainspecStatus>(json).unwrap(),
            status
        );
    }

    #[test]
    fn status_preserves_upgrade_information_when_chainspec_discovery_fails() {
        let next_upgrade = NextUpgrade::new(
            casper_types::ActivationPoint::EraId(EraId::new(100)),
            ProtocolVersion::from_parts(2, 2, 0),
        );
        let feed = StatusFeed::new(
            None,
            BTreeMap::new(),
            ChainspecInfo::new(
                "casper-test".to_string(),
                Some(next_upgrade),
                Err("unreadable installed chainspec".to_string()).into(),
            ),
            None,
            Duration::ZERO,
            ReactorState::KeepUp,
            Timestamp::zero(),
            AvailableBlockRange::new(0, 0),
            BlockSynchronizerStatus::new(None, None),
            Digest::default(),
            None,
        );
        let status = GetStatusResult::new(feed, ProtocolVersion::from_parts(2, 1, 0));
        assert_eq!(status.next_upgrade, Some(next_upgrade));
        let json = serde_json::to_value(status).unwrap();
        assert_eq!(
            json["chainspecs"]["error"],
            "unreadable installed chainspec"
        );
        assert_eq!(json["chainspec_name"], "casper-test");
    }
}
