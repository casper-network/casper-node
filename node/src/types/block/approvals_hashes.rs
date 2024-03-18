use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
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

use crate::{
    components::{
        contract_runtime::APPROVALS_CHECKSUM_NAME,
        fetcher::{FetchItem, Tag},
    },
    types::{self, VariantMismatch},
};

use casper_storage::global_state::trie_store::operations::compute_state_hash;

/// The data which is gossiped by validators to non-validators upon creation of a new block.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ApprovalsHashes {
    /// Hash of the block that contains transactions that are relevant to the approvals.
    block_hash: BlockHash,
    /// The set of all transactions' finalized approvals' hashes.
    approvals_hashes: Vec<ApprovalsHash>,
    /// The Merkle proof of the checksum registry containing the checksum of the finalized
    /// approvals.
    #[data_size(skip)]
    merkle_proof_approvals: TrieMerkleProof<Key, StoredValue>,
}

impl ApprovalsHashes {
    #[allow(dead_code)]
    pub(crate) fn new(
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

    fn verify(&self, block: &Block) -> Result<(), ApprovalsHashesValidationError> {
        let merkle_proof_approvals = &self.merkle_proof_approvals;
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
            Block::V1(v1_block) => compute_legacy_approvals_checksum(self.deploy_ids(v1_block))?,
            Block::V2(v2_block) => {
                types::compute_approvals_checksum(self.transaction_ids(v2_block))
                    .map_err(ApprovalsHashesValidationError::ApprovalsChecksum)?
            }
        };

        if value_in_proof != computed_approvals_checksum {
            return Err(ApprovalsHashesValidationError::ApprovalsChecksumMismatch {
                computed_approvals_checksum,
                value_in_proof,
            });
        }

        Ok(())
    }

    pub(crate) fn deploy_ids(&self, v1_block: &BlockV1) -> Vec<DeployId> {
        let approval_hashes = &self.approvals_hashes;
        v1_block
            .deploy_and_transfer_hashes()
            .zip(approval_hashes)
            .map(|(x, y)| DeployId::new(*x, *y))
            .collect()
    }

    pub fn transaction_ids(&self, v2_block: &BlockV2) -> Vec<TransactionId> {
        let approval_hashes = &self.approvals_hashes;
        v2_block
            .all_transactions()
            .zip(approval_hashes)
            .map(|(x, y)| TransactionId::new(*x, *y))
            .collect()
    }

    pub(crate) fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }
}

impl FetchItem for ApprovalsHashes {
    type Id = BlockHash;
    type ValidationError = ApprovalsHashesValidationError;
    type ValidationMetadata = Block;

    const TAG: Tag = Tag::ApprovalsHashes;

    fn fetch_id(&self) -> Self::Id {
        *self.block_hash()
    }

    fn validate(&self, block: &Block) -> Result<(), Self::ValidationError> {
        self.verify(block)
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

    #[error(transparent)]
    #[data_size(skip)]
    VariantMismatch(#[from] VariantMismatch),
}

mod specimen_support {
    use std::collections::BTreeMap;

    use casper_types::{
        bytesrepr::Bytes,
        global_state::{Pointer, TrieMerkleProof, TrieMerkleProofStep},
        CLValue, Digest, Key, StoredValue,
    };

    use crate::{
        contract_runtime::{APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME},
        utils::specimen::{
            largest_variant, vec_of_largest_specimen, vec_prop_specimen, Cache, LargestSpecimen,
            SizeEstimator,
        },
    };
    use casper_storage::block_store::types::ApprovalsHashes;

    impl LargestSpecimen for ApprovalsHashes {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            let data = {
                let mut map = BTreeMap::new();
                map.insert(
                    APPROVALS_CHECKSUM_NAME,
                    Digest::largest_specimen(estimator, cache),
                );
                map.insert(
                    EXECUTION_RESULTS_CHECKSUM_NAME,
                    Digest::largest_specimen(estimator, cache),
                );
                map
            };
            let merkle_proof_approvals = TrieMerkleProof::new(
                Key::ChecksumRegistry,
                StoredValue::CLValue(CLValue::from_t(data).expect("a correct cl value")),
                // 2^64/2^13 = 2^51, so 51 items:
                vec_of_largest_specimen(estimator, 51, cache).into(),
            );
            ApprovalsHashes::new(
                LargestSpecimen::largest_specimen(estimator, cache),
                vec_prop_specimen(estimator, "approvals_hashes", cache),
                merkle_proof_approvals,
            )
        }
    }

    impl LargestSpecimen for TrieMerkleProofStep {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            #[derive(strum::EnumIter)]
            enum TrieMerkleProofStepDiscriminants {
                Node,
                Extension,
            }

            largest_variant(estimator, |variant| match variant {
                TrieMerkleProofStepDiscriminants::Node => TrieMerkleProofStep::Node {
                    hole_index: u8::MAX,
                    indexed_pointers_with_hole: vec![
                        (
                            u8::MAX,
                            Pointer::LeafPointer(LargestSpecimen::largest_specimen(
                                estimator, cache
                            ))
                        );
                        estimator.parameter("max_pointer_per_node")
                    ],
                },
                TrieMerkleProofStepDiscriminants::Extension => TrieMerkleProofStep::Extension {
                    affix: Bytes::from(vec![u8::MAX; Key::max_serialized_length()]),
                },
            })
        }
    }
}
