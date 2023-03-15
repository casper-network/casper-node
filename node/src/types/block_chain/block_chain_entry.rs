use crate::types::{BlockHash, BlockHeader, DataSize};
use casper_types::EraId;
use std::cmp::Ordering;

/// Lightweight index entry for blocks across relevant lifecycle states.
#[derive(DataSize, Debug, Copy, Clone, PartialEq)]
pub enum BlockChainEntry {
    Vacant {
        block_height: u64,
    },
    Proposed {
        block_height: u64,
    },
    Finalized {
        block_height: u64,
        era_id: EraId,
    },
    Incomplete {
        block_height: u64,
        block_hash: BlockHash,
        era_id: EraId,
    },
    Complete {
        block_height: u64,
        block_hash: BlockHash,
        era_id: EraId,
        is_switch_block: bool,
    },
}

// todo!("remove allow unused once integrated");
#[allow(unused)]
impl BlockChainEntry {
    /// Create a vacant item
    pub fn vacant(block_height: u64) -> Self {
        BlockChainEntry::Vacant { block_height }
    }

    /// Create a new proposed item.
    pub fn new_proposed(block_height: u64) -> Self {
        BlockChainEntry::Proposed { block_height }
    }

    /// Create a new finalize item.
    pub fn new_finalized(block_height: u64, era_id: EraId) -> Self {
        BlockChainEntry::Finalized {
            block_height,
            era_id,
        }
    }

    /// Create a new incomplete item.
    pub fn new_incomplete(block_height: u64, block_hash: BlockHash, era_id: EraId) -> Self {
        BlockChainEntry::Incomplete {
            block_height,
            block_hash,
            era_id,
        }
    }

    /// Create a new complete item.
    pub fn new_complete(block_header: &BlockHeader) -> Self {
        BlockChainEntry::Complete {
            block_height: block_header.height(),
            block_hash: block_header.block_hash(),
            era_id: block_header.era_id(),
            is_switch_block: block_header.is_switch_block(),
        }
    }

    /// Get block height from item
    pub fn block_height(&self) -> u64 {
        match self {
            BlockChainEntry::Vacant { block_height }
            | BlockChainEntry::Proposed { block_height }
            | BlockChainEntry::Finalized { block_height, .. }
            | BlockChainEntry::Incomplete { block_height, .. }
            | BlockChainEntry::Complete { block_height, .. } => *block_height,
        }
    }

    /// Get block hash from item, if present.
    pub fn block_hash(&self) -> Option<BlockHash> {
        match self {
            BlockChainEntry::Vacant { .. }
            | BlockChainEntry::Proposed { .. }
            | BlockChainEntry::Finalized { .. } => None,
            BlockChainEntry::Incomplete { block_hash, .. }
            | BlockChainEntry::Complete { block_hash, .. } => Some(*block_hash),
        }
    }

    /// Get era id from item, if present.
    pub fn era_id(&self) -> Option<EraId> {
        match self {
            BlockChainEntry::Vacant { .. } | BlockChainEntry::Proposed { .. } => None,
            BlockChainEntry::Finalized { era_id, .. }
            | BlockChainEntry::Incomplete { era_id, .. }
            | BlockChainEntry::Complete { era_id, .. } => Some(*era_id),
        }
    }

    /// Is this instance finalized or higher and in the imputed era?
    pub fn is_definitely_in_era(&self, era_id: EraId) -> bool {
        match self.era_id() {
            Some(eid) => eid.eq(&era_id),
            None => false,
        }
    }

    /// Is this instance a switch block?
    pub fn is_switch_block(&self) -> Option<bool> {
        if let BlockChainEntry::Complete {
            is_switch_block, ..
        } = self
        {
            Some(*is_switch_block)
        } else {
            None
        }
    }

    /// Is this instance complete and a switch block?
    pub fn is_complete_switch_block(&self) -> bool {
        match self {
            BlockChainEntry::Vacant { .. }
            | BlockChainEntry::Proposed { .. }
            | BlockChainEntry::Finalized { .. }
            | BlockChainEntry::Incomplete { .. } => false,
            BlockChainEntry::Complete {
                is_switch_block, ..
            } => *is_switch_block,
        }
    }

    /// All non-vacant entries.
    pub fn all_non_vacant(&self) -> bool {
        match self {
            BlockChainEntry::Vacant { .. } => false,
            BlockChainEntry::Proposed { .. }
            | BlockChainEntry::Finalized { .. }
            | BlockChainEntry::Incomplete { .. }
            | BlockChainEntry::Complete { .. } => true,
        }
    }

    /// All entries.
    pub fn all(&self) -> bool {
        true
    }

    /// Is this instance the vacant variant?
    pub fn is_vacant(&self) -> bool {
        matches!(self, BlockChainEntry::Vacant { .. })
    }

    /// Is this instance the proposed variant?
    pub fn is_proposed(&self) -> bool {
        matches!(self, BlockChainEntry::Proposed { .. })
    }

    /// Is this instance the finalized variant?
    pub fn is_finalized(&self) -> bool {
        matches!(self, BlockChainEntry::Finalized { .. })
    }

    /// Is this instance the incomplete variant?
    pub fn is_incomplete(&self) -> bool {
        matches!(self, BlockChainEntry::Incomplete { .. })
    }

    /// Is this instance the complete variant?
    pub fn is_complete(&self) -> bool {
        matches!(self, BlockChainEntry::Complete { .. })
    }

    pub fn status(&self) -> u8 {
        match self {
            BlockChainEntry::Vacant { .. } => 0,
            BlockChainEntry::Proposed { .. } => 1,
            BlockChainEntry::Finalized { .. } => 2,
            BlockChainEntry::Incomplete { .. } => 3,
            BlockChainEntry::Complete { .. } => 4,
        }
    }

    pub fn higher_status(&self, other: &BlockChainEntry) -> Ordering {
        self.status().cmp(&other.status())
    }

    pub fn higher_height_and_status(&self, other: &BlockChainEntry) -> Ordering {
        let height_order = self.block_height().cmp(&other.block_height());
        if height_order == Ordering::Greater {
            return self.status().cmp(&other.status());
        }
        height_order
    }
}
