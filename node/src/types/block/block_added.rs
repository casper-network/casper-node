use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof;
use casper_hashing::Digest;
use casper_types::bytesrepr::ToBytes;
use casper_types::{bytesrepr, Key, StoredValue};

use super::{Block, BlockHash};
use crate::types::error::BlockCreationError;
use crate::{
    components::contract_runtime::APPROVALS_CHECKSUM_NAME,
    effect::GossipTarget,
    types::{Approval, BlockValidationError, FetcherItem, GossiperItem, Item, Tag},
};

/// An error that can arise when validating a `BlockAdded`.
#[derive(Error, Debug, Serialize)]
#[non_exhaustive]
pub enum BlockAddedValidationError {
    /// The key provided in the proof is not a `Key::ChecksumRegistry`.
    #[error("key provided in proof is not a Key::ChecksumRegistry")]
    InvalidKeyType,

    /// An error while validating the `block` field.
    #[error(transparent)]
    BlockValidationError(#[from] BlockValidationError),

    /// An error while computing the state root hash implied by the Merkle proof.
    #[error("failed to compute state root hash implied by proof")]
    TrieMerkleProof(bytesrepr::Error),

    /// The state root hash implied by the Merkle proof doesn't match that in the block.
    #[error("state root hash implied by the Merkle proof doesn't match that in the block")]
    StateRootHashMismatch {
        proof_state_root_hash: Digest,
        block_state_root_hash: Digest,
    },

    /// The value provided in the proof cannot be parsed to the checksum registry type.
    #[error("value provided in the proof cannot be parsed to the checksum registry type")]
    InvalidChecksumRegistry,

    /// An error while computing the root hash of the approvals.
    #[error("failed to compute root hash of the approvals")]
    ApprovalsRootHash(bytesrepr::Error),

    /// The approvals root hash implied by the Merkle proof doesn't match the approvals.
    #[error("approvals root hash implied by the Merkle proof doesn't match the approvals")]
    ApprovalsRootHashMismatch {
        computed_approvals_root_hash: Digest,
        value_in_proof: Digest,
    },
}

/// The data which is gossiped by validators to non-validators upon creation of a new block.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockAdded {
    /// The block.
    pub block: Block,
    /// The set of all deploys' finalized approvals in the order in which they appear in the block.
    pub finalized_approvals: Vec<BTreeSet<Approval>>,
    /// The Merkle proof of the finalized approvals.
    #[data_size(skip)]
    pub merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
}

impl Item for BlockAdded {
    type Id = BlockHash;
    const TAG: Tag = Tag::BlockAdded;

    fn id(&self) -> Self::Id {
        *self.block.hash()
    }
}

impl FetcherItem for BlockAdded {
    type ValidationError = BlockAddedValidationError;
    type ValidationMetadata = ();

    fn validate(&self, _metadata: &()) -> Result<(), Self::ValidationError> {
        if *self.merkle_proof_approvals.key() != Key::ChecksumRegistry {
            return Err(BlockAddedValidationError::InvalidKeyType);
        }

        self.block.validate(&())?;

        let proof_state_root_hash = self
            .merkle_proof_approvals
            .compute_state_hash()
            .map_err(BlockAddedValidationError::TrieMerkleProof)?;

        if proof_state_root_hash != *self.block.state_root_hash() {
            return Err(BlockAddedValidationError::StateRootHashMismatch {
                proof_state_root_hash,
                block_state_root_hash: *self.block.state_root_hash(),
            });
        }

        let value_in_proof = self
            .merkle_proof_approvals
            .value()
            .as_cl_value()
            .and_then(|cl_value| cl_value.clone().into_t().ok())
            .and_then(|registry: BTreeMap<String, Digest>| {
                registry.get(APPROVALS_CHECKSUM_NAME).copied()
            })
            .ok_or_else(|| BlockAddedValidationError::InvalidChecksumRegistry)?;

        let computed_approvals_root_hash = {
            let mut approval_hashes = vec![];
            for approvals in &self.finalized_approvals {
                let bytes = approvals
                    .to_bytes()
                    .map_err(BlockAddedValidationError::ApprovalsRootHash)?;
                approval_hashes.push(Digest::hash(bytes));
            }
            Digest::hash_merkle_tree(approval_hashes)
        };

        if value_in_proof != computed_approvals_root_hash {
            return Err(BlockAddedValidationError::ApprovalsRootHashMismatch {
                computed_approvals_root_hash,
                value_in_proof,
            });
        }

        Ok(())
    }
}

impl GossiperItem for BlockAdded {
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn target(&self) -> GossipTarget {
        GossipTarget::NonValidators(self.block.header.era_id)
    }
}

impl Display for BlockAdded {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block added: {}", self.block.hash())
    }
}

// on receipt of gossiped ^
// * check if we have the deploys - if we do, add the finalized approvals to storage
// * if missing deploy, fetch deploy and store
