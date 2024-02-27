use serde::Deserialize;

use casper_types::{
    global_state::TrieMerkleProof, BlockHash, DeployApprovalsHash, Key, StoredValue,
};

use crate::types::ApprovalsHashes;

/// Initial version of `ApprovalsHashes` prior to `casper-node` v2.0.0.
#[derive(Deserialize)]
pub(super) struct LegacyApprovalsHashes {
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
