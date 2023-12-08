use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    AvailableBlockRange, Block, BlockHash, BlockSynchronizerStatus, Digest, EraId, NextUpgrade,
    Peers, PublicKey, ReactorState, TimeDiff, Timestamp,
};
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Status information about the node.
#[derive(Debug)]
pub struct NodeStatus {
    /// The node ID and network address of each connected peer.
    pub peers: Peers,
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
}

impl FromBytes for NodeStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (peers, remainder) = FromBytes::from_bytes(bytes)?;
        let (build_version, remainder) = String::from_bytes(remainder)?;
        let (chainspec_name, remainder) = String::from_bytes(remainder)?;
        let (starting_state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (last_added_block_info, remainder) = Option::<MinimalBlockInfo>::from_bytes(remainder)?;
        let (our_public_signing_key, remainder) = Option::<PublicKey>::from_bytes(remainder)?;
        let (round_length, remainder) = Option::<TimeDiff>::from_bytes(remainder)?;
        let (next_upgrade, remainder) = Option::<NextUpgrade>::from_bytes(remainder)?;
        let (uptime, remainder) = TimeDiff::from_bytes(remainder)?;
        let (reactor_state, remainder) = ReactorState::from_bytes(remainder)?;
        let (last_progress, remainder) = Timestamp::from_bytes(remainder)?;
        let (available_block_range, remainder) = AvailableBlockRange::from_bytes(remainder)?;
        let (block_sync, remainder) = BlockSynchronizerStatus::from_bytes(remainder)?;
        Ok((
            NodeStatus {
                peers,
                build_version,
                chainspec_name,
                starting_state_root_hash,
                last_added_block_info,
                our_public_signing_key,
                round_length,
                next_upgrade,
                uptime,
                reactor_state,
                last_progress,
                available_block_range,
                block_sync,
            },
            remainder,
        ))
    }
}

impl ToBytes for NodeStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let NodeStatus {
            peers,
            build_version,
            chainspec_name,
            starting_state_root_hash,
            last_added_block_info,
            our_public_signing_key,
            round_length,
            next_upgrade,
            uptime,
            reactor_state,
            last_progress,
            available_block_range,
            block_sync,
        } = self;
        peers.write_bytes(writer)?;
        build_version.write_bytes(writer)?;
        chainspec_name.write_bytes(writer)?;
        starting_state_root_hash.write_bytes(writer)?;
        last_added_block_info.write_bytes(writer)?;
        our_public_signing_key.write_bytes(writer)?;
        round_length.write_bytes(writer)?;
        next_upgrade.write_bytes(writer)?;
        uptime.write_bytes(writer)?;
        reactor_state.write_bytes(writer)?;
        last_progress.write_bytes(writer)?;
        available_block_range.write_bytes(writer)?;
        block_sync.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.peers.serialized_length()
            + self.build_version.serialized_length()
            + self.chainspec_name.serialized_length()
            + self.starting_state_root_hash.serialized_length()
            + self.last_added_block_info.serialized_length()
            + self.our_public_signing_key.serialized_length()
            + self.round_length.serialized_length()
            + self.next_upgrade.serialized_length()
            + self.uptime.serialized_length()
            + self.reactor_state.serialized_length()
            + self.last_progress.serialized_length()
            + self.available_block_range.serialized_length()
            + self.block_sync.serialized_length()
    }
}

/// Minimal info of a `Block`.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(any(feature = "std", test), serde(deny_unknown_fields))]
pub struct MinimalBlockInfo {
    hash: BlockHash,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    state_root_hash: Digest,
    creator: PublicKey,
}

impl FromBytes for MinimalBlockInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (height, remainder) = u64::from_bytes(remainder)?;
        let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (creator, remainder) = PublicKey::from_bytes(remainder)?;
        Ok((
            MinimalBlockInfo {
                hash,
                timestamp,
                era_id,
                height,
                state_root_hash,
                creator,
            },
            remainder,
        ))
    }
}

impl ToBytes for MinimalBlockInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.height.write_bytes(writer)?;
        self.state_root_hash.write_bytes(writer)?;
        self.creator.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.timestamp.serialized_length()
            + self.era_id.serialized_length()
            + self.height.serialized_length()
            + self.state_root_hash.serialized_length()
            + self.creator.serialized_length()
    }
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
