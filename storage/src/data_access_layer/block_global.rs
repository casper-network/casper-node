use crate::tracking_copy::TrackingCopyError;
use casper_types::{execution::Effects, BlockHash, BlockTime, Digest, ProtocolVersion};
use std::fmt::{Display, Formatter};
use thiserror::Error;

/// Block global kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BlockGlobalKind {
    /// Block time.
    BlockTime(BlockTime),
    /// Message count.
    MessageCount(u64),
    /// Protocol version.
    ProtocolVersion(ProtocolVersion),
    /// Addressable entity flag.
    AddressableEntity(bool),
    /// EIP-4788 parent block hash record.
    Eip4788ParentHash {
        /// EVM block timestamp in seconds.
        timestamp_secs: u64,
        /// Parent block hash associated with the timestamp.
        parent_hash: BlockHash,
    },
}

impl Default for BlockGlobalKind {
    fn default() -> Self {
        BlockGlobalKind::BlockTime(BlockTime::default())
    }
}

/// Block global request.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct BlockGlobalRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_global_kind: BlockGlobalKind,
}

impl BlockGlobalRequest {
    /// Returns block time setting request.
    pub fn block_time(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
    ) -> Self {
        let block_global_kind = BlockGlobalKind::BlockTime(block_time);
        BlockGlobalRequest {
            state_hash,
            protocol_version,
            block_global_kind,
        }
    }

    /// Returns protocol version setting request.
    pub fn set_protocol_version(state_hash: Digest, protocol_version: ProtocolVersion) -> Self {
        let block_global_kind = BlockGlobalKind::ProtocolVersion(protocol_version);
        BlockGlobalRequest {
            state_hash,
            protocol_version,
            block_global_kind,
        }
    }

    /// Returns addressable entity flag setting request.
    pub fn set_addressable_entity(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        addressable_entity: bool,
    ) -> Self {
        let block_global_kind = BlockGlobalKind::AddressableEntity(addressable_entity);
        BlockGlobalRequest {
            state_hash,
            protocol_version,
            block_global_kind,
        }
    }

    /// Returns an EIP-4788 parent block hash setting request.
    pub fn set_eip4788_parent_hash(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        timestamp_secs: u64,
        parent_hash: BlockHash,
    ) -> Self {
        let block_global_kind = BlockGlobalKind::Eip4788ParentHash {
            timestamp_secs,
            parent_hash,
        };
        BlockGlobalRequest {
            state_hash,
            protocol_version,
            block_global_kind,
        }
    }

    /// Returns state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns block global kind.
    pub fn block_global_kind(&self) -> BlockGlobalKind {
        self.block_global_kind
    }
}

/// Block global result.
#[derive(Error, Debug, Clone)]
pub enum BlockGlobalResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// Failed to store block global data.
    Failure(TrackingCopyError),
    /// Successfully stored block global data.
    Success {
        /// State hash after data committed to the global state.
        post_state_hash: Digest,
        /// The effects of putting the data to global state.
        effects: Box<Effects>,
    },
}

impl Display for BlockGlobalResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockGlobalResult::RootNotFound => f.write_str("root not found"),
            BlockGlobalResult::Failure(tce) => {
                write!(f, "failed {}", tce)
            }
            BlockGlobalResult::Success { .. } => f.write_str("success"),
        }
    }
}
