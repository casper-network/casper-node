use datasize::DataSize;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::types::{Block, BlockExecutionResultsOrChunkId, BlockHash, DeployHash, DeployId};

use super::execution_results_acquisition::ExecutionResultsChecksum;

#[derive(DataSize, Debug, Clone)]
pub(crate) enum NeedNext {
    Nothing,
    Peers(BlockHash),
    EraValidators(EraId),
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    ApprovalsHashes(BlockHash, Box<Block>),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(BlockHash, Digest),
    DeployByHash(BlockHash, DeployHash),
    DeployById(BlockHash, DeployId),
    /// We want the Merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    ExecutionResultsRootHash(BlockHash, Digest),
    ExecutionResults(
        BlockHash,
        BlockExecutionResultsOrChunkId,
        ExecutionResultsChecksum,
    ),
    MarkComplete(BlockHash, u64),
}
