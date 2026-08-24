use crate::tracking_copy::TrackingCopyError;
use casper_types::{execution::Effects, BlockTime, Digest, ProtocolVersion};
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
    /// Block information.
    BlockInfo {
        /// Block time.
        block_time: BlockTime,
        /// Protocol version.
        protocol_version: ProtocolVersion,
        /// Addressable entity enabled.
        addressable_entity_enabled: bool,
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

    /// Returns block info setting request.
    pub fn set_block_info(
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        addressable_entity_enabled: bool,
    ) -> Self {
        let block_global_kind = BlockGlobalKind::BlockInfo {
            block_time,
            protocol_version,
            addressable_entity_enabled,
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

impl BlockGlobalResult {
    /// Returns true if successful, else false.
    pub fn is_success(&self) -> bool {
        match self {
            BlockGlobalResult::RootNotFound | BlockGlobalResult::Failure(_) => false,
            BlockGlobalResult::Success { .. } => true,
        }
    }

    /// Returns post_state_hash if available.
    pub fn post_state_hash(&self) -> Option<Digest> {
        match self {
            BlockGlobalResult::RootNotFound | BlockGlobalResult::Failure(_) => None,
            BlockGlobalResult::Success {
                post_state_hash, ..
            } => Some(*post_state_hash),
        }
    }
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
