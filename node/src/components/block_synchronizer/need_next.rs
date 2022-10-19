use datasize::DataSize;
use itertools::Either;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::types::{Block, BlockExecutionResultsOrChunkId, BlockHash, DeployHash, DeployId};

use super::execution_results_acquisition::ExecutionResultsChecksum;

#[derive(DataSize, Debug, Clone)]
pub(crate) enum NeedNext {
    NothingNeededSufficientFinalitySignaturesWeight(BlockHash),
    Nothing,
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    ApprovalsHashes(BlockHash, Box<Block>),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(BlockHash, Digest),
    DeployByHash(BlockHash, DeployHash),
    DeployById(BlockHash, DeployId),
    /// We want the Merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    ExecutionResultsRootHash {
        block_hash: BlockHash,
        global_state_root_hash: Digest,
    },
    // todo!(): Isn't the block hash already in the ID?
    ExecutionResults(
        BlockHash,
        BlockExecutionResultsOrChunkId,
        ExecutionResultsChecksum,
    ),
    EraValidators(EraId),
    Peers(BlockHash),
}
