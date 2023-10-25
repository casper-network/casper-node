use datasize::DataSize;
use derive_more::Display;

use casper_types::{Block, BlockHash, DeployHash, DeployId, Digest, EraId, PublicKey};

use crate::types::{BlockExecutionResultsOrChunkId, ExecutableBlock};

use super::execution_results_acquisition::ExecutionResultsChecksum;

#[derive(DataSize, Debug, Clone, Display, PartialEq)]
pub(crate) enum NeedNext {
    #[display(fmt = "need next for {}: nothing", _0)]
    Nothing(BlockHash),
    #[display(fmt = "need next for {}: peers", _0)]
    Peers(BlockHash),
    #[display(fmt = "need next for {}: era validators", _0)]
    EraValidators(EraId),
    #[display(fmt = "need next for {}: block header", _0)]
    BlockHeader(BlockHash),
    #[display(fmt = "need next for {}: block body", _0)]
    BlockBody(BlockHash),
    #[display(fmt = "need next for {}: approvals hashes ({})", _0, _1)]
    ApprovalsHashes(BlockHash, Box<Block>),
    #[display(
        fmt = "need next for {}: finality signatures at {} ({} validators)",
        _0,
        _1,
        "_2.len()"
    )]
    FinalitySignatures(BlockHash, EraId, Vec<PublicKey>),
    #[display(fmt = "need next for {}: global state (state root hash {})", _0, _1)]
    GlobalState(BlockHash, Digest),
    #[display(fmt = "need next for {}: deploy {}", _0, _1)]
    DeployByHash(BlockHash, DeployHash),
    #[display(fmt = "need next for {}: deploy {}", _0, _1)]
    DeployById(BlockHash, DeployId),
    #[display(fmt = "need next for {}: make block executable (height {})", _0, _1)]
    MakeExecutableBlock(BlockHash, u64),
    #[display(
        fmt = "need next for {}: enqueue this block (height {}) for execution",
        _0,
        _1
    )]
    EnqueueForExecution(BlockHash, u64, Box<ExecutableBlock>),
    /// We want the Merkle root hash stored in global state under the ChecksumRegistry key for the
    /// execution results.
    #[display(
        fmt = "need next for {}: execution results checksum (state root hash {})",
        _0,
        _1
    )]
    ExecutionResultsChecksum(BlockHash, Digest),
    #[display(fmt = "need next for {}: {} (checksum {})", _0, _1, _2)]
    ExecutionResults(
        BlockHash,
        BlockExecutionResultsOrChunkId,
        ExecutionResultsChecksum,
    ),
    #[display(fmt = "need next for {}: mark complete (height {})", _0, _1)]
    BlockMarkedComplete(BlockHash, u64),
    #[display(
        fmt = "need next for {}: transition acquisition state to HaveStrictFinality (height {})",
        _0,
        _1
    )]
    SwitchToHaveStrictFinality(BlockHash, u64),
}
