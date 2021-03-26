use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::PublicKey;

use crate::{
    components::consensus::traits::ConsensusValueT,
    crypto::hash::Digest,
    types::{ProtoBlock, Timestamp},
};

/// A proposed block. Once the consensus protocol reaches agreement on it, it will be converted to
/// a `FinalizedBlock`.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct CandidateBlock {
    proto_block: ProtoBlock,
    accusations: Vec<PublicKey>,
    /// The parent candidate block in this era, or `None` if this is the first block in this era.
    parent: Option<Digest>,
}

impl CandidateBlock {
    /// Creates a new candidate block, wrapping a proto block and accusing the given validators.
    pub(crate) fn new(
        proto_block: ProtoBlock,
        accusations: Vec<PublicKey>,
        parent: Option<Digest>,
    ) -> Self {
        CandidateBlock {
            proto_block,
            accusations,
            parent,
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
    type Hash = Digest;

    fn hash(&self) -> Self::Hash {
        let CandidateBlock {
            proto_block,
            accusations,
            parent,
        } = self;
        let mut result = [0; Digest::LENGTH];

        let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");
        hasher.update(proto_block.hash().inner());
        let data = (accusations, parent);
        hasher.update(bincode::serialize(&data).expect("should serialize candidate block data"));
        hasher.finalize_variable(|slice| {
            result.copy_from_slice(slice);
        });
        result.into()
    }

    fn needs_validation(&self) -> bool {
        !self.proto_block.wasm_deploys().is_empty() || !self.proto_block.transfers().is_empty()
    }

    fn timestamp(&self) -> Timestamp {
        self.proto_block.timestamp()
    }

    fn parent(&self) -> Option<&Self::Hash> {
        self.parent.as_ref()
    }
}
