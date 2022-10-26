use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64),
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::BlockIdentifier(hash, _) => *hash,
            StartingWith::SyncedBlockIdentifier(hash, _) => *hash,
            StartingWith::ExecutableBlock(hash, _) => *hash,
            StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn should_fetch_execution_state(&self) -> bool {
        match self {
            StartingWith::ExecutableBlock(..) => false,
            StartingWith::BlockIdentifier(..)
            | StartingWith::SyncedBlockIdentifier(..)
            | StartingWith::Hash(_) => true,
        }
    }
}
