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
            StartingWith::LocalTip(hash, _)
            | StartingWith::BlockIdentifier(hash, _)
            | StartingWith::SyncedBlockIdentifier(hash, _)
            | StartingWith::ExecutableBlock(hash, _)
            | StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::LocalTip(_, height)
            | StartingWith::BlockIdentifier(_, height)
            | StartingWith::SyncedBlockIdentifier(_, height)
            | StartingWith::ExecutableBlock(_, height) => Some(*height),
            StartingWith::Hash(_) => None,
        }
    }

    pub(crate) fn is_historical(&self) -> bool {
        match self {
            StartingWith::ExecutableBlock(..) => false,
            StartingWith::LocalTip(..)
            | StartingWith::SyncedBlockIdentifier(..)
            | StartingWith::BlockIdentifier(..)
            | StartingWith::Hash(_) => true,
        }
    }

    pub(crate) fn is_executable_block(&self) -> bool {
        matches!(self, StartingWith::ExecutableBlock(_, _))
    }

    pub(crate) fn is_synced_block_identifier(&self) -> bool {
        matches!(self, StartingWith::SyncedBlockIdentifier(_, _))
    }

    pub(crate) fn is_local_tip(&self) -> bool {
        matches!(self, StartingWith::LocalTip(_, _))
    }
}
