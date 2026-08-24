mod approvals_hashes;
mod block_hash_height_and_era;
mod deploy_metadata_v1;
mod transfers;

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
};

pub use approvals_hashes::{ApprovalsHashes, ApprovalsHashesValidationError};
pub use block_hash_height_and_era::BlockHashHeightAndEra;
use casper_types::{execution::ExecutionResult, Approval, BlockHash, TransactionHash, Transfer};

pub(crate) use approvals_hashes::LegacyApprovalsHashes;
pub(crate) use deploy_metadata_v1::DeployMetadataV1;
pub(in crate::block_store) use transfers::Transfers;

/// Transaction finalized approvals.
pub struct TransactionFinalizedApprovals {
    /// Transaction hash.
    pub transaction_hash: TransactionHash,
    /// Finalized approvals.
    pub finalized_approvals: BTreeSet<Approval>,
}

/// Block execution results.
pub struct BlockExecutionResults {
    /// Block info.
    pub block_info: BlockHashHeightAndEra,
    /// Execution results.
    pub exec_results: HashMap<TransactionHash, ExecutionResult>,
}

/// Block transfers.
pub struct BlockTransfers {
    /// Block hash.
    pub block_hash: BlockHash,
    /// Transfers.
    pub transfers: Vec<Transfer>,
}

/// State store.
pub struct StateStore {
    /// Key.
    pub key: Cow<'static, [u8]>,
    /// Value.
    pub value: Vec<u8>,
}

/// State store key.
pub struct StateStoreKey(pub(super) Cow<'static, [u8]>);

impl StateStoreKey {
    /// Ctor.
    pub fn new(key: Cow<'static, [u8]>) -> Self {
        StateStoreKey(key)
    }
}

/// Block tip anchor.
pub(crate) struct Tip;

/// Latest switch block anchor.
pub(crate) struct LatestSwitchBlock;

/// Block height.
pub(crate) type BlockHeight = u64;
