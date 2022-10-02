use datasize::DataSize;
use itertools::Either;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::types::{BlockExecutionResultsOrChunkId, BlockHash, DeployHash, DeployId};

#[derive(DataSize, Debug, Clone)]
pub(crate) enum NeedNext {
    Nothing,
    BlockHeader(BlockHash),
    BlockBody(BlockHash),
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    GlobalState(BlockHash, Digest),
    // TODO - don't skip
    #[data_size(skip)]
    Deploy(BlockHash, Either<DeployHash, DeployId>),
    /// We want the Merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    ExecutionResultsRootHash {
        global_state_root_hash: Digest,
    },
    ExecutionResults(BlockHash, BlockExecutionResultsOrChunkId),
    EraValidators(EraId),
    Peers(BlockHash),
}
