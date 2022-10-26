use crate::types::BlockHash;

#[derive(Debug)]
pub(crate) enum SyncInstruction {
    Leap,
    CaughtUp,
    BlockExec {
        next_block_hash: Option<BlockHash>,
    },
    BlockSync {
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
    },
}
