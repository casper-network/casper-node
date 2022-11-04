use casper_types::EraId;

use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64),
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64),
    LocalTip(BlockHash, u64, EraId),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::LocalTip(hash, _, _)
            | StartingWith::BlockIdentifier(hash, _)
            | StartingWith::SyncedBlockIdentifier(hash, _)
            | StartingWith::ExecutableBlock(hash, _)
            | StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::LocalTip(_, height, _)
            | StartingWith::BlockIdentifier(_, height)
            | StartingWith::SyncedBlockIdentifier(_, height)
            | StartingWith::ExecutableBlock(_, height) => Some(*height),
            StartingWith::Hash(_) => None,
        }
    }

    pub(crate) fn is_executable_block(&self) -> bool {
        matches!(self, StartingWith::ExecutableBlock(_, _))
    }

    pub(crate) fn is_synced_block_identifier(&self) -> bool {
        matches!(self, StartingWith::SyncedBlockIdentifier(_, _))
    }
}
