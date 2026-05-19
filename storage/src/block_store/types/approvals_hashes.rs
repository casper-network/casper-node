use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    global_state::TrieMerkleProof,
    ApprovalsHash, Block, BlockHash, BlockV1, BlockV2, DeployId, Digest, Key, StoredValue,
    TransactionId,
};

use crate::global_state::trie_store::operations::compute_state_hash;

pub(crate) const APPROVALS_CHECKSUM_NAME: &str = "approvals_checksum";

/// Returns the hash of the bytesrepr-encoded deploy_ids.
fn compute_approvals_checksum(txn_ids: Vec<TransactionId>) -> Result<Digest, bytesrepr::Error> {
    let bytes = txn_ids.into_bytes()?;
    Ok(Digest::hash(bytes))
}

/// The data which is gossiped by validators to non-validators upon creation of a new block.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalsHashes {
    /// Hash of the block that contains deploys that are relevant to the approvals.
    block_hash: BlockHash,
    /// The set of all deploys' finalized approvals' hashes.
    approvals_hashes: Vec<ApprovalsHash>,
    /// The Merkle proof of the checksum registry containing the checksum of the finalized
    /// approvals.
    #[data_size(skip)]
    merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
}

impl ApprovalsHashes {
    /// Ctor.
    pub fn new(
        block_hash: BlockHash,
        approvals_hashes: Vec<ApprovalsHash>,
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    ) -> Self {
        Self {
            block_hash,
            approvals_hashes,
            merkle_proof_approvals,
        }
    }

    /// Verify block.
    pub fn verify(&self, block: &Block) -> Result<(), ApprovalsHashesValidationError> {
        if *self.merkle_proof_approvals.key() != Key::ChecksumRegistry {
            return Err(ApprovalsHashesValidationError::InvalidKeyType);
        }

        let proof_state_root_hash = compute_state_hash(&self.merkle_proof_approvals)
            .map_err(ApprovalsHashesValidationError::TrieMerkleProof)?;

        if proof_state_root_hash != *block.state_root_hash() {
            return Err(ApprovalsHashesValidationError::StateRootHashMismatch {
                proof_state_root_hash,
                block_state_root_hash: *block.state_root_hash(),
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

        let computed_approvals_checksum = match block {
            Block::V1(v1_block) => compute_legacy_approvals_checksum(self.deploy_ids(v1_block)?)?,
            Block::V2(v2_block) => compute_approvals_checksum(self.transaction_ids(v2_block)?)
                .map_err(ApprovalsHashesValidationError::ApprovalsChecksum)?,
        };

        if value_in_proof != computed_approvals_checksum {
            return Err(ApprovalsHashesValidationError::ApprovalsChecksumMismatch {
                computed_approvals_checksum,
                value_in_proof,
            });
        }

        Ok(())
    }

    /// Deploy ids.
    pub(crate) fn deploy_ids(
        &self,
        v1_block: &BlockV1,
    ) -> Result<Vec<DeployId>, ApprovalsHashesValidationError> {
        let deploy_approvals_hashes = self.approvals_hashes.clone();
        Ok(v1_block
            .deploy_and_transfer_hashes()
            .zip(deploy_approvals_hashes)
            .map(|(deploy_hash, deploy_approvals_hash)| {
                DeployId::new(*deploy_hash, deploy_approvals_hash)
            })
            .collect())
    }

    /// Transaction ids.
    pub fn transaction_ids(
        &self,
        v2_block: &BlockV2,
    ) -> Result<Vec<TransactionId>, ApprovalsHashesValidationError> {
        v2_block
            .all_transactions()
            .zip(self.approvals_hashes.clone())
            .map(|(txn_hash, txn_approvals_hash)| {
                Ok(TransactionId::new(*txn_hash, txn_approvals_hash))
            })
            .collect()
    }

    /// Block hash.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    /// Approvals hashes.
    pub fn approvals_hashes(&self) -> Vec<ApprovalsHash> {
        self.approvals_hashes.clone()
    }
}

impl Display for ApprovalsHashes {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "approvals hashes for {}", self.block_hash())
    }
}

impl ToBytes for ApprovalsHashes {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(writer)?;
        self.approvals_hashes.write_bytes(writer)?;
        self.merkle_proof_approvals.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.approvals_hashes.serialized_length()
            + self.merkle_proof_approvals.serialized_length()
    }
}

impl FromBytes for ApprovalsHashes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (approvals_hashes, remainder) = Vec::<ApprovalsHash>::from_bytes(remainder)?;
        let (merkle_proof_approvals, remainder) =
            TrieMerkleProof::<Key, StoredValue>::from_bytes(remainder)?;
        Ok((
            ApprovalsHashes {
                block_hash,
                approvals_hashes,
                merkle_proof_approvals,
            },
            remainder,
        ))
    }
}

/// Returns the hash of the bytesrepr-encoded deploy_ids, as used until the `Block` enum became
/// available.
pub(crate) fn compute_legacy_approvals_checksum(
    deploy_ids: Vec<DeployId>,
) -> Result<Digest, ApprovalsHashesValidationError> {
    let bytes = deploy_ids
        .into_bytes()
        .map_err(ApprovalsHashesValidationError::ApprovalsChecksum)?;
    Ok(Digest::hash(bytes))
}

/// An error that can arise when validating `ApprovalsHashes`.
#[derive(Error, Debug, DataSize)]
#[non_exhaustive]
pub enum ApprovalsHashesValidationError {
    /// The key provided in the proof is not a `Key::ChecksumRegistry`.
    #[error("key provided in proof is not a Key::ChecksumRegistry")]
    InvalidKeyType,

    /// An error while computing the state root hash implied by the Merkle proof.
    #[error("failed to compute state root hash implied by proof")]
    TrieMerkleProof(bytesrepr::Error),

    /// The state root hash implied by the Merkle proof doesn't match that in the block.
    #[error("state root hash implied by the Merkle proof doesn't match that in the block")]
    StateRootHashMismatch {
        /// Proof state root hash.
        proof_state_root_hash: Digest,
        /// Block state root hash.
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
        /// Computed approvals checksum.
        computed_approvals_checksum: Digest,
        /// Value in proof.
        value_in_proof: Digest,
    },

    /// Variant mismatch.
    #[error("mismatch in variants: {0:?}")]
    #[data_size(skip)]
    VariantMismatch(Box<dyn Debug + Send + Sync>),
}

/// Initial version of `ApprovalsHashes` prior to `casper-node` v2.0.0.
#[derive(Deserialize)]
pub(crate) struct LegacyApprovalsHashes {
    block_hash: BlockHash,
    approvals_hashes: Vec<ApprovalsHash>,
    merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
}

impl From<LegacyApprovalsHashes> for ApprovalsHashes {
    fn from(
        LegacyApprovalsHashes {
            block_hash,
            approvals_hashes,
            merkle_proof_approvals,
        }: LegacyApprovalsHashes,
    ) -> Self {
        ApprovalsHashes::new(block_hash, approvals_hashes, merkle_proof_approvals)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use casper_types::{
        global_state::TrieMerkleProof, testing::TestRng, ApprovalsHash, CLValue, ChecksumRegistry,
        StoredValue, TestBlockBuilder, Transaction,
    };

    use super::*;

    fn approvals_hashes_for_block(
        transactions: &[Transaction],
        block: &Block,
    ) -> Vec<ApprovalsHash> {
        let transaction_approvals_hashes: BTreeMap<_, _> = transactions
            .iter()
            .map(|transaction| {
                (
                    transaction.hash(),
                    transaction
                        .compute_approvals_hash()
                        .expect("should compute approvals hash"),
                )
            })
            .collect();

        match block {
            Block::V1(_) => unreachable!("test helper builds V2 blocks"),
            Block::V2(v2_block) => v2_block
                .all_transactions()
                .map(|txn_hash| {
                    *transaction_approvals_hashes
                        .get(txn_hash)
                        .expect("transaction should be in block")
                })
                .collect(),
        }
    }

    fn approvals_hashes_with_proof(
        rng: &mut TestRng,
        transactions: &[Transaction],
        supplied_approvals_hashes: Vec<ApprovalsHash>,
        registry_approvals_hashes: Vec<ApprovalsHash>,
    ) -> (Block, ApprovalsHashes) {
        let ordering_block = TestBlockBuilder::new()
            .transactions(transactions)
            .build_versioned(rng);
        let transaction_hashes: Vec<_> = match &ordering_block {
            Block::V1(_) => unreachable!("test helper builds V2 blocks"),
            Block::V2(v2_block) => v2_block.all_transactions().copied().collect(),
        };
        let transaction_ids = transaction_hashes
            .into_iter()
            .zip(registry_approvals_hashes)
            .map(|(txn_hash, txn_approvals_hash)| TransactionId::new(txn_hash, txn_approvals_hash))
            .collect();
        let approvals_checksum =
            compute_approvals_checksum(transaction_ids).expect("should compute checksum");

        let mut checksum_registry = ChecksumRegistry::new();
        checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);
        let merkle_proof_approvals = TrieMerkleProof::new(
            Key::ChecksumRegistry,
            StoredValue::CLValue(
                CLValue::from_t(checksum_registry).expect("should build checksum registry"),
            ),
            VecDeque::new(),
        );
        let state_root_hash =
            compute_state_hash(&merkle_proof_approvals).expect("should compute state root hash");

        let block = TestBlockBuilder::new()
            .state_root_hash(state_root_hash)
            .transactions(transactions)
            .build_versioned(rng);

        let approvals_hashes = ApprovalsHashes::new(
            *block.hash(),
            supplied_approvals_hashes,
            merkle_proof_approvals,
        );

        (block, approvals_hashes)
    }

    #[test]
    fn verify_rejects_extra_approvals_hashes() {
        let rng = &mut TestRng::new();
        let transactions: Vec<_> = (0..3).map(|_| Transaction::random(rng)).collect();
        let ordering_block = TestBlockBuilder::new()
            .transactions(&transactions)
            .build_versioned(rng);
        let valid_approvals_hashes = approvals_hashes_for_block(&transactions, &ordering_block);

        let mut supplied_approvals_hashes = valid_approvals_hashes.clone();
        supplied_approvals_hashes.push(ApprovalsHash::from(Digest::from([42; Digest::LENGTH])));
        let (block, approvals_hashes) = approvals_hashes_with_proof(
            rng,
            &transactions,
            supplied_approvals_hashes,
            valid_approvals_hashes,
        );

        let result = approvals_hashes.verify(&block);
        assert!(
            result.is_err(),
            "verify must reject overlong ApprovalsHashes, got {:?}",
            result,
        );
    }

    #[test]
    fn verify_rejects_missing_approvals_hashes() {
        let rng = &mut TestRng::new();
        let transactions: Vec<_> = (0..3).map(|_| Transaction::random(rng)).collect();
        let ordering_block = TestBlockBuilder::new()
            .transactions(&transactions)
            .build_versioned(rng);
        let valid_approvals_hashes = approvals_hashes_for_block(&transactions, &ordering_block);

        let supplied_approvals_hashes = valid_approvals_hashes[..2].to_vec();
        let (block, approvals_hashes) = approvals_hashes_with_proof(
            rng,
            &transactions,
            supplied_approvals_hashes,
            valid_approvals_hashes,
        );

        let result = approvals_hashes.verify(&block);
        assert!(
            result.is_err(),
            "verify must reject missing ApprovalsHashes entries, got {:?}",
            result,
        );
    }
}
