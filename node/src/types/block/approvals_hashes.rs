use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof;
use casper_hashing::Digest;
use casper_types::{bytesrepr, Key, StoredValue};

use super::{Block, BlockHash};
use crate::{
    components::contract_runtime::APPROVALS_CHECKSUM_NAME,
    types::{self, ApprovalsHash, DeployId, FetcherItem, Item, Tag},
    utils::ds,
};

/// The data which is gossiped by validators to non-validators upon creation of a new block.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ApprovalsHashes {
    // Hash of the block that contains deploys that are relevant to the approvals.
    block_hash: BlockHash,
    /// The set of all deploys' finalized approvals' hashes.
    approvals_hashes: Vec<ApprovalsHash>,
    /// The Merkle proof of the checksum registry containing the checksum of
    /// the finalized approvals.
    #[data_size(skip)]
    merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    is_verified: OnceCell<Result<(), ApprovalsHashesValidationError>>,
}

impl ApprovalsHashes {
    pub(crate) fn new(
        block_hash: &BlockHash,
        approvals_hashes: Vec<ApprovalsHash>,
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    ) -> Self {
        Self {
            block_hash: *block_hash,
            approvals_hashes,
            merkle_proof_approvals,
            is_verified: OnceCell::new(),
        }
    }

    fn verify(&self, block: &Block) -> Result<(), ApprovalsHashesValidationError> {
        if *self.merkle_proof_approvals.key() != Key::ChecksumRegistry {
            return Err(ApprovalsHashesValidationError::InvalidKeyType);
        }

        let proof_state_root_hash = self
            .merkle_proof_approvals
            .compute_state_hash()
            .map_err(ApprovalsHashesValidationError::TrieMerkleProof)?;

        if proof_state_root_hash != *block.header().state_root_hash() {
            return Err(ApprovalsHashesValidationError::StateRootHashMismatch {
                proof_state_root_hash,
                block_state_root_hash: *block.header().state_root_hash(),
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
            .ok_or(ApprovalsHashesValidationError::InvalidChecksumRegistry)?;

        let computed_approvals_checksum =
            types::compute_approvals_checksum(self.deploy_ids(block).collect())
                .map_err(ApprovalsHashesValidationError::ApprovalsChecksum)?;

        if value_in_proof != computed_approvals_checksum {
            return Err(ApprovalsHashesValidationError::ApprovalsChecksumMismatch {
                computed_approvals_checksum,
                value_in_proof,
            });
        }

        Ok(())
    }

    pub(crate) fn deploy_ids<'a>(
        &'a self,
        block: &'a Block,
    ) -> impl Iterator<Item = DeployId> + 'a {
        block
            .deploy_and_transfer_hashes()
            .zip(&self.approvals_hashes)
            .map(|(deploy_hash, approvals_hash)| DeployId::new(*deploy_hash, *approvals_hash))
    }

    #[allow(dead_code)]
    pub(crate) fn approvals_hashes(&self) -> &[ApprovalsHash] {
        self.approvals_hashes.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn merkle_proof_approvals(&self) -> &TrieMerkleProof<Key, StoredValue> {
        &self.merkle_proof_approvals
    }

    pub(crate) fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }
}

impl Item for ApprovalsHashes {
    type Id = BlockHash;
    const TAG: Tag = Tag::ApprovalsHashes;

    fn id(&self) -> Self::Id {
        self.block_hash
    }
}

impl FetcherItem for ApprovalsHashes {
    type ValidationError = ApprovalsHashesValidationError;
    type ValidationMetadata = Block;

    fn validate(&self, block: &Block) -> Result<(), Self::ValidationError> {
        self.is_verified.get_or_init(|| self.verify(block)).clone()
    }
}

impl Display for ApprovalsHashes {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "approvals hashes for block: {}", self.block_hash)
    }
}

/// An error that can arise when validating `ApprovalsHashes`.
#[derive(Error, Clone, Debug, PartialEq, Eq, DataSize)]
#[non_exhaustive]
pub(crate) enum ApprovalsHashesValidationError {
    /// The key provided in the proof is not a `Key::ChecksumRegistry`.
    #[error("key provided in proof is not a Key::ChecksumRegistry")]
    InvalidKeyType,

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

    /// An error while computing the checksum of the approvals.
    #[error("failed to compute checksum of the approvals")]
    ApprovalsChecksum(bytesrepr::Error),

    /// The approvals checksum provided doesn't match one calculated from the approvals.
    #[error("provided approvals checksum doesn't match one calculated from the approvals")]
    ApprovalsChecksumMismatch {
        computed_approvals_checksum: Digest,
        value_in_proof: Digest,
    },
}
