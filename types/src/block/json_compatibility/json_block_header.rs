#[cfg(feature = "datasize")]
use datasize::DataSize;
use once_cell::sync::OnceCell;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    super::{BlockHash, BlockHeader, EraEnd},
    JsonEraEnd,
};
use crate::{Digest, EraId, ProtocolVersion, Timestamp};

/// A JSON-friendly representation of [`BlockHeader`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(description = "The header portion of a block.")]
#[serde(deny_unknown_fields)]
pub struct JsonBlockHeader {
    /// The parent block's hash.
    pub parent_hash: BlockHash,
    /// The root hash of global state after the deploys in this block have been executed.
    pub state_root_hash: Digest,
    /// The hash of the block's body.
    pub body_hash: Digest,
    /// A random bit needed for initializing a future era.
    pub random_bit: bool,
    /// A seed needed for initializing a future era.
    pub accumulated_seed: Digest,
    /// The `EraEnd` of a block if it is a switch block.
    pub era_end: Option<JsonEraEnd>,
    /// The timestamp from when the block was proposed.
    pub timestamp: Timestamp,
    /// The era ID in which this block was created.
    pub era_id: EraId,
    /// The height of this block, i.e. the number of ancestors.
    pub height: u64,
    /// The protocol version of the network from when this block was created.
    pub protocol_version: ProtocolVersion,
}

impl From<BlockHeader> for JsonBlockHeader {
    fn from(block_header: BlockHeader) -> Self {
        JsonBlockHeader {
            parent_hash: block_header.parent_hash,
            state_root_hash: block_header.state_root_hash,
            body_hash: block_header.body_hash,
            random_bit: block_header.random_bit,
            accumulated_seed: block_header.accumulated_seed,
            era_end: block_header.era_end.map(JsonEraEnd::from),
            timestamp: block_header.timestamp,
            era_id: block_header.era_id,
            height: block_header.height,
            protocol_version: block_header.protocol_version,
        }
    }
}

impl From<JsonBlockHeader> for BlockHeader {
    fn from(block_header: JsonBlockHeader) -> Self {
        BlockHeader {
            parent_hash: block_header.parent_hash,
            state_root_hash: block_header.state_root_hash,
            body_hash: block_header.body_hash,
            random_bit: block_header.random_bit,
            accumulated_seed: block_header.accumulated_seed,
            era_end: block_header.era_end.map(EraEnd::from),
            timestamp: block_header.timestamp,
            era_id: block_header.era_id,
            height: block_header.height,
            protocol_version: block_header.protocol_version,
            block_hash: OnceCell::new(),
        }
    }
}
