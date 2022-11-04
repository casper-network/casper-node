use casper_types::EraId;

use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    ExecutableBlock(BlockHash, u64),
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64),
    LocalTip {
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        is_last_block_before_activation: bool,
    },
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::LocalTip { block_hash, .. }
            | StartingWith::BlockIdentifier(block_hash, _)
            | StartingWith::SyncedBlockIdentifier(block_hash, _)
            | StartingWith::ExecutableBlock(block_hash, _)
            | StartingWith::Hash(block_hash) => *block_hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::LocalTip { block_height, .. }
            | StartingWith::BlockIdentifier(_, block_height)
            | StartingWith::SyncedBlockIdentifier(_, block_height)
            | StartingWith::ExecutableBlock(_, block_height) => Some(*block_height),
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
