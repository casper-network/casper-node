use datasize::DataSize;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::types::{BlockExecutionResultsOrChunkId, BlockHash, DeployHash};

#[derive(DataSize, Debug, Clone)]
pub(crate) enum NeedNext {
    Nothing,
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(BlockHash, Digest),
    Deploy(BlockHash, DeployHash),
    /// We want the merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    ExecutionResultsRootHash {
        global_state_root_hash: Digest,
    },
    ExecutionResults(BlockHash, BlockExecutionResultsOrChunkId),
    EraValidators(EraId),
    Peers(BlockHash),
}
