use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::traits::ConsensusValueT, crypto::asymmetric_key::PublicKey,
    types::ProtoBlock,
};

/// A proposed block. Once the consensus protocol reaches agreement on it, it will be converted to
/// a `FinalizedBlock`.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct CandidateBlock {
    proto_block: ProtoBlock,
    accusations: Vec<PublicKey>,
}

impl CandidateBlock {
    /// Creates a new candidate block, wrapping a proto block and accusing the given validators.
    pub(crate) fn new(proto_block: ProtoBlock, accusations: Vec<PublicKey>) -> Self {
        CandidateBlock {
            proto_block,
            accusations,
        }
    }

    /// Returns the proto block containing the deploys.
    pub(crate) fn proto_block(&self) -> &ProtoBlock {
        &self.proto_block
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
    fn is_empty(&self) -> bool {
        self.proto_block.wasm_deploys().is_empty() && self.proto_block.transfers().is_empty()
    }
}
