mod approvals_hashes;
mod block_hash_height_and_era;
mod deploy_metadata_v1;
mod transfers;

use std::{borrow::Cow, collections::HashMap};

pub use approvals_hashes::{ApprovalsHashes, ApprovalsHashesValidationError};
pub use block_hash_height_and_era::BlockHashHeightAndEra;
use casper_types::{
    execution::ExecutionResult, Block, BlockHash, BlockHeader, FinalizedApprovals, TransactionHash,
    Transfer,
};

pub(crate) use approvals_hashes::LegacyApprovalsHashes;
pub(crate) use deploy_metadata_v1::DeployMetadataV1;
pub(in crate::block_store) use transfers::Transfers;

pub type ExecutionResults = HashMap<TransactionHash, ExecutionResult>;

pub struct TransactionFinalizedApprovals {
    pub transaction_hash: TransactionHash,
    pub finalized_approvals: FinalizedApprovals,
}

pub struct BlockExecutionResults {
    pub block_info: BlockHashHeightAndEra,
    pub exec_results: ExecutionResults,
}

pub struct BlockTransfers {
    pub block_hash: BlockHash,
    pub transfers: Vec<Transfer>,
}

pub struct StateStore {
    pub key: Cow<'static, [u8]>,
    pub value: Vec<u8>,
}

pub struct StateStoreKey(pub(super) Cow<'static, [u8]>);

impl StateStoreKey {
    pub fn new(key: Cow<'static, [u8]>) -> Self {
        StateStoreKey(key)
    }
}

pub struct Tip;
pub struct LatestSwitchBlock;

pub type BlockHeight = u64;
pub type SwitchBlockHeader = BlockHeader;
pub type SwitchBlock = Block;
