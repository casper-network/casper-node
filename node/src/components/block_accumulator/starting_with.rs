use casper_types::EraId;

use crate::types::BlockHash;

#[derive(Clone, Debug)]
pub(crate) enum StartingWith {
    BlockIdentifier(BlockHash, u64),
    SyncedBlockIdentifier(BlockHash, u64, EraId),
    LocalTip(BlockHash, u64, EraId),
    Hash(BlockHash),
}

impl StartingWith {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            StartingWith::BlockIdentifier(hash, _)
            | StartingWith::SyncedBlockIdentifier(hash, _, _)
            | StartingWith::LocalTip(hash, _, _)
            | StartingWith::Hash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            StartingWith::BlockIdentifier(_, height)
            | StartingWith::SyncedBlockIdentifier(_, height, _)
            | StartingWith::LocalTip(_, height, _) => Some(*height),
            StartingWith::Hash(_) => None,
        }
    }

    pub(crate) fn is_held_locally(&self) -> bool {
        match self {
            StartingWith::Hash(_) | StartingWith::BlockIdentifier(_, _) => false,
            StartingWith::SyncedBlockIdentifier(_, _, _) | StartingWith::LocalTip(_, _, _) => true,
        }
    }

    pub(crate) fn maybe_local_tip_identifier(&self) -> Option<(u64, EraId)> {
        match self {
            StartingWith::Hash(_) | StartingWith::BlockIdentifier(_, _) => None,
            StartingWith::SyncedBlockIdentifier(_, block_height, era_id)
            | StartingWith::LocalTip(_, block_height, era_id) => Some((*block_height, *era_id)),
        }
    }
}
