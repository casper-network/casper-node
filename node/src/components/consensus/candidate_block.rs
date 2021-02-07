use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::PublicKey;

use crate::{
    components::consensus::traits::ConsensusValueT,
    types::{ProtoBlock, Timestamp},
};

/// A proposed block. Once the consensus protocol reaches agreement on it, it will be converted to
/// a `FinalizedBlock`.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct CandidateBlock {
    proto_block: ProtoBlock,
    timestamp: Timestamp,
    accusations: Vec<PublicKey>,
}

impl CandidateBlock {
    /// Creates a new candidate block, wrapping a proto block and accusing the given validators.
    pub(crate) fn new(
        proto_block: ProtoBlock,
        timestamp: Timestamp,
        accusations: Vec<PublicKey>,
    ) -> Self {
        CandidateBlock {
            proto_block,
            timestamp,
            accusations,
        }
    }

    /// Returns the proto block containing the deploys.
    pub(crate) fn proto_block(&self) -> &ProtoBlock {
        &self.proto_block
    }

    /// Returns the candidate block's timestamp, i.e. when the block was proposed.
    ///
    /// This is identical to the timestamp of the Highway unit, and the timestamp of the `Block`,
    /// if it gets finalized.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the validators accused by this block.
    pub(crate) fn accusations(&self) -> &Vec<PublicKey> {
        &self.accusations
    }
}

impl From<CandidateBlock> for ProtoBlock {
    fn from(cb: CandidateBlock) -> ProtoBlock {
        cb.proto_block
    }
}

impl ConsensusValueT for CandidateBlock {
    fn needs_validation(&self) -> bool {
        !self.proto_block.wasm_deploys().is_empty() || !self.proto_block.transfers().is_empty()
    }
}
