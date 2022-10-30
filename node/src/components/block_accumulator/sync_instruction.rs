use crate::types::BlockHash;

#[derive(Debug)]
pub(crate) enum SyncInstruction {
    Leap {
        block_hash: BlockHash,
    },
    BlockExec {
        block_hash: BlockHash,
        next_block_hash: Option<BlockHash>,
    },
    BlockSync {
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
    },
    CaughtUp,
}
