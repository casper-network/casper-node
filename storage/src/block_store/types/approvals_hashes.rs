use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    global_state::TrieMerkleProof,
    Block, BlockHash, BlockV1, BlockV2, DeployApprovalsHash, DeployId, Digest, Key, StoredValue,
    TransactionApprovalsHash, TransactionHash, TransactionId,
};

use crate::global_state::trie_store::operations::compute_state_hash;

pub(crate) const APPROVALS_CHECKSUM_NAME: &str = "approvals_checksum";

const V1_TAG: u8 = 0;
const V2_TAG: u8 = 1;

/// Returns the hash of the bytesrepr-encoded deploy_ids.
fn compute_approvals_checksum(txn_ids: Vec<TransactionId>) -> Result<Digest, bytesrepr::Error> {
    let bytes = txn_ids.into_bytes()?;
    Ok(Digest::hash(bytes))
}

/// The data which is gossiped by validators to non-validators upon creation of a new block.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalsHashes {
    #[serde(rename = "Version1")]
    V1 {
        /// Hash of the block that contains deploys that are relevant to the approvals.
        block_hash: BlockHash,
        /// The set of all deploys' finalized approvals' hashes.
        approvals_hashes: Vec<DeployApprovalsHash>,
        /// The Merkle proof of the checksum registry containing the checksum of the finalized
        /// approvals.
        #[data_size(skip)]
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    },
    #[serde(rename = "Version2")]
    V2 {
        /// Hash of the block that contains transactions that are relevant to the approvals.
        block_hash: BlockHash,
        /// The set of all transactions' finalized approvals' hashes.
        approvals_hashes: Vec<TransactionApprovalsHash>,
        /// The Merkle proof of the checksum registry containing the checksum of the finalized
        /// approvals.
        #[data_size(skip)]
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    },
}

impl ApprovalsHashes {
    pub fn new_v1(
        block_hash: BlockHash,
        approvals_hashes: Vec<DeployApprovalsHash>,
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    ) -> Self {
        Self::V1 {
            block_hash,
            approvals_hashes,
            merkle_proof_approvals,
        }
    }

    pub fn new_v2(
        block_hash: BlockHash,
        approvals_hashes: Vec<TransactionApprovalsHash>,
        merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
    ) -> Self {
        Self::V2 {
            block_hash,
            approvals_hashes,
            merkle_proof_approvals,
        }
    }

    pub fn verify(&self, block: &Block) -> Result<(), ApprovalsHashesValidationError> {
        let merkle_proof_approvals = match self {
            ApprovalsHashes::V1 {
                merkle_proof_approvals,
                ..
            } => merkle_proof_approvals,
            ApprovalsHashes::V2 {
                merkle_proof_approvals,
                ..
            } => merkle_proof_approvals,
        };
        if *merkle_proof_approvals.key() != Key::ChecksumRegistry {
            return Err(ApprovalsHashesValidationError::InvalidKeyType);
        }

        let proof_state_root_hash = compute_state_hash(merkle_proof_approvals)
            .map_err(ApprovalsHashesValidationError::TrieMerkleProof)?;

        if proof_state_root_hash != *block.state_root_hash() {
            return Err(ApprovalsHashesValidationError::StateRootHashMismatch {
                proof_state_root_hash,
                block_state_root_hash: *block.state_root_hash(),
            });
        }

        let value_in_proof = merkle_proof_approvals
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

    pub(crate) fn deploy_ids(
        &self,
        v1_block: &BlockV1,
    ) -> Result<Vec<DeployId>, ApprovalsHashesValidationError> {
        let deploy_approvals_hashes = match self {
            ApprovalsHashes::V1 {
                approvals_hashes, ..
            } => approvals_hashes,
            txn_approvals_hashes => {
                let mismatch = ApprovalsHashesValidationError::VariantMismatch(Box::new((
                    txn_approvals_hashes.clone(),
                    v1_block.clone(),
                )));
                return Err(mismatch);
            }
        };
        Ok(v1_block
            .deploy_and_transfer_hashes()
            .zip(deploy_approvals_hashes)
            .map(|(deploy_hash, deploy_approvals_hash)| {
                DeployId::new(*deploy_hash, *deploy_approvals_hash)
            })
            .collect())
    }

    pub fn transaction_ids(
        &self,
        v2_block: &BlockV2,
    ) -> Result<Vec<TransactionId>, ApprovalsHashesValidationError> {
        let txn_approvals_hashes = match self {
            ApprovalsHashes::V2 {
                approvals_hashes, ..
            } => approvals_hashes,
            deploy_approvals_hashes => {
                let mismatch = ApprovalsHashesValidationError::VariantMismatch(Box::new((
                    deploy_approvals_hashes.clone(),
                    v2_block.clone(),
                )));
                return Err(mismatch);
            }
        };

        v2_block
            .all_transactions()
            .zip(txn_approvals_hashes)
            .map(
                |(txn_hash, txn_approvals_hash)| match (txn_hash, txn_approvals_hash) {
                    (
                        TransactionHash::Deploy(deploy_hash),
                        TransactionApprovalsHash::Deploy(deploy_approvals_hash),
                    ) => Ok(TransactionId::new_deploy(
                        *deploy_hash,
                        *deploy_approvals_hash,
                    )),
                    (
                        TransactionHash::V1(v1_hash),
                        TransactionApprovalsHash::V1(v1_approvals_hash),
                    ) => Ok(TransactionId::new_v1(*v1_hash, *v1_approvals_hash)),
                    (txn_hash, txn_approvals_hash) => {
                        let mismatch = ApprovalsHashesValidationError::VariantMismatch(Box::new((
                            *txn_hash,
                            *txn_approvals_hash,
                        )));
                        Err(mismatch)
                    }
                },
            )
            .collect()
    }

    pub fn block_hash(&self) -> &BlockHash {
        match self {
            ApprovalsHashes::V1 { block_hash, .. } => block_hash,
            ApprovalsHashes::V2 { block_hash, .. } => block_hash,
        }
    }

    pub fn approvals_hashes(&self) -> Vec<TransactionApprovalsHash> {
        match self {
            ApprovalsHashes::V1 {
                approvals_hashes, ..
            } => approvals_hashes
                .iter()
                .map(|deploy_approvals_hash| {
                    TransactionApprovalsHash::Deploy(*deploy_approvals_hash)
                })
                .collect(),
            ApprovalsHashes::V2 {
                approvals_hashes, ..
            } => approvals_hashes.clone(),
        }
    }
}

impl Display for ApprovalsHashes {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "approvals hashes for {}", self.block_hash())
    }
}

impl ToBytes for ApprovalsHashes {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ApprovalsHashes::V1 {
                block_hash,
                approvals_hashes,
                merkle_proof_approvals,
            } => {
                V1_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)?;
                approvals_hashes.write_bytes(writer)?;
                merkle_proof_approvals.write_bytes(writer)
            }
            ApprovalsHashes::V2 {
                block_hash,
                approvals_hashes,
                merkle_proof_approvals,
            } => {
                V2_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)?;
                approvals_hashes.write_bytes(writer)?;
                merkle_proof_approvals.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ApprovalsHashes::V1 {
                    block_hash,
                    approvals_hashes,
                    merkle_proof_approvals,
                } => {
                    block_hash.serialized_length()
                        + approvals_hashes.serialized_length()
                        + merkle_proof_approvals.serialized_length()
                }
                ApprovalsHashes::V2 {
                    block_hash,
                    approvals_hashes,
                    merkle_proof_approvals,
                } => {
                    block_hash.serialized_length()
                        + approvals_hashes.serialized_length()
                        + merkle_proof_approvals.serialized_length()
                }
            }
    }
}

impl FromBytes for ApprovalsHashes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            V1_TAG => {
                let (block_hash, remainder) = BlockHash::from_bytes(remainder)?;
                let (approvals_hashes, remainder) =
                    Vec::<DeployApprovalsHash>::from_bytes(remainder)?;
                let (merkle_proof_approvals, remainder) =
                    TrieMerkleProof::<Key, StoredValue>::from_bytes(remainder)?;
                let v1_approvals_hashes = ApprovalsHashes::V1 {
                    block_hash,
                    approvals_hashes,
                    merkle_proof_approvals,
                };
                Ok((v1_approvals_hashes, remainder))
            }
            V2_TAG => {
                let (block_hash, remainder) = BlockHash::from_bytes(remainder)?;
                let (approvals_hashes, remainder) =
                    Vec::<TransactionApprovalsHash>::from_bytes(remainder)?;
                let (merkle_proof_approvals, remainder) =
                    TrieMerkleProof::<Key, StoredValue>::from_bytes(remainder)?;
                let v2_approvals_hashes = ApprovalsHashes::V2 {
                    block_hash,
                    approvals_hashes,
                    merkle_proof_approvals,
                };
                Ok((v2_approvals_hashes, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
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

    #[error("mismatch in variants: {0:?}")]
    #[data_size(skip)]
    VariantMismatch(Box<dyn Debug + Send + Sync>),
}

/// Initial version of `ApprovalsHashes` prior to `casper-node` v2.0.0.
#[derive(Deserialize)]
pub(crate) struct LegacyApprovalsHashes {
    block_hash: BlockHash,
    approvals_hashes: Vec<DeployApprovalsHash>,
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
        ApprovalsHashes::new_v1(block_hash, approvals_hashes, merkle_proof_approvals)
    }
}
