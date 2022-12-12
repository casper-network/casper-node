use crate::types::BlockHash;

#[derive(Debug)]
pub(crate) enum SyncInstruction {
    Leap { block_hash: BlockHash },
    BlockSync { block_hash: BlockHash },
    CaughtUp { block_hash: BlockHash },
}

impl SyncInstruction {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            SyncInstruction::Leap { block_hash }
            | SyncInstruction::BlockSync { block_hash }
            | SyncInstruction::CaughtUp { block_hash } => *block_hash,
        }
    }
}
