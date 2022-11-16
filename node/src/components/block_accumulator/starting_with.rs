use casper_types::EraId;

use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64, EraId),
    ExecutableBlock(BlockHash, u64, EraId),
    LocalTip(BlockHash, u64, EraId),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::ExecutableBlock(hash, _, _)
            | StartingWith::BlockIdentifier(hash, _)
            | StartingWith::SyncedBlockIdentifier(hash, _, _)
            | StartingWith::LocalTip(hash, _, _)
            | StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::ExecutableBlock(_, height, _)
            | StartingWith::BlockIdentifier(_, height)
            | StartingWith::SyncedBlockIdentifier(_, height, _)
            | StartingWith::LocalTip(_, height, _) => Some(*height),
            StartingWith::Hash(_) => None,
        }
    }

    pub(crate) fn is_executable_block(&self) -> bool {
        matches!(self, StartingWith::ExecutableBlock(_, _, _))
    }

    pub(crate) fn is_synced_block_identifier(&self) -> bool {
        matches!(self, StartingWith::SyncedBlockIdentifier(_, _, _))
    }

    pub(crate) fn maybe_local_tip_identifier(&self) -> Option<(u64, EraId)> {
        match self {
            StartingWith::Hash(_)
            | StartingWith::BlockIdentifier(_, _)
            | StartingWith::ExecutableBlock(_, _, _) => None,
            StartingWith::SyncedBlockIdentifier(_, block_height, era_id)
            | StartingWith::LocalTip(_, block_height, era_id) => Some((*block_height, *era_id)),
        }
    }
}
