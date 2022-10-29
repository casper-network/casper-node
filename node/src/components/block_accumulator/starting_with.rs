use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64),
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64),
    LocalTip(BlockHash, u64),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::LocalTip(hash, _) => *hash,
            StartingWith::BlockIdentifier(hash, _) => *hash,
            StartingWith::SyncedBlockIdentifier(hash, _) => *hash,
            StartingWith::ExecutableBlock(hash, _) => *hash,
            StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn is_executable(&self) -> bool {
        match self {
            StartingWith::ExecutableBlock(..) => false,
            StartingWith::LocalTip(..)
            | StartingWith::SyncedBlockIdentifier(..)
            | StartingWith::BlockIdentifier(..)
            | StartingWith::Hash(_) => true,
        }
    }
}
