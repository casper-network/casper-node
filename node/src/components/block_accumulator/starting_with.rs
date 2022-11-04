use casper_types::EraId;

use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64, Option<EraId>),
    BlockIdentifier(BlockHash, u64, Option<EraId>),
    SyncedBlockIdentifier(BlockHash, u64, Option<EraId>),
    LocalTip(BlockHash, u64, EraId),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::LocalTip(hash, _, _)
            | StartingWith::BlockIdentifier(hash, _, _)
            | StartingWith::SyncedBlockIdentifier(hash, _, _)
            | StartingWith::ExecutableBlock(hash, _, _)
            | StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::LocalTip(_, height, _)
            | StartingWith::BlockIdentifier(_, height, _)
            | StartingWith::SyncedBlockIdentifier(_, height, _)
            | StartingWith::ExecutableBlock(_, height, _) => Some(*height),
            StartingWith::Hash(_) => None,
        }
    }

    pub(crate) fn is_executable_block(&self) -> bool {
        matches!(self, StartingWith::ExecutableBlock(_, _, _))
    }

    pub(crate) fn is_synced_block_identifier(&self) -> bool {
        matches!(self, StartingWith::SyncedBlockIdentifier(_, _, _))
    }
}
