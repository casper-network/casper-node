use crate::types::{
    block_chain::{BlockChainEntry, Error},
    BlockHash, BlockHeader,
};
use casper_types::EraId;

/// API for a block chain representation with block_height indexed lookup.
pub trait IndexedBlockChain {
    /// Remove an item by block height.
    fn remove(&mut self, block_height: u64) -> Option<BlockChainEntry>;
    /// Register a block that is proposed.
    fn register_proposed(&mut self, block_height: u64) -> Result<(), Error>;
    /// Register a block that is finalized.
    fn register_finalized(&mut self, block_height: u64, era_id: EraId) -> Result<(), Error>;
    /// Register a block that is not yet complete.
    fn register_incomplete(&mut self, block_height: u64, block_hash: BlockHash, era_id: EraId);
    /// Register a block that has been marked complete.
    fn register_complete(&mut self, block_header: &BlockHeader);
    /// Returns entry by height, if present.
    fn by_height(&self, block_height: u64) -> BlockChainEntry;
    /// Return entry by hash, if present.
    fn by_hash(&self, block_hash: &BlockHash) -> Option<BlockChainEntry>;
    /// Returns entry of child, if present.
    fn by_parent(&self, parent_block_hash: &BlockHash) -> Option<BlockChainEntry>;
    /// Returns entry of parent, if present.
    fn by_child(&self, child_block_hash: &BlockHash) -> Option<BlockChainEntry>;
    /// Is switch block and is in imputed era?
    fn switch_block_by_era(&self, era_id: EraId) -> Option<BlockChainEntry>;
    /// Is block at height incomplete?
    fn is_incomplete(&self, block_height: u64) -> bool;
    /// Is block at height complete?
    fn is_complete(&self, block_height: u64) -> bool;
    /// Is block at height a switch block?
    fn is_switch_block(&self, block_height: u64) -> Option<bool>;
    /// Is block at height an immediate switch block?
    fn is_immediate_switch_block(&self, block_height: u64) -> Option<bool>;
    /// Returns the lowest entry (by block height) where the predicate is true, if any.
    fn lowest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns the highest entry (by block height) where the predicate is true, if any.
    fn highest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns the highest entry (by block height) where the predicate is true, if any.
    fn higher_by_status<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns the lowest switch block entry, if any.
    fn lowest_switch_block(&self) -> Option<BlockChainEntry>;
    /// Returns the highest switch block entry, if any.
    fn highest_switch_block(&self) -> Option<BlockChainEntry>;
    /// Returns all actual entries where the predicate is true, if any.
    fn all_by<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns the highest entry (by block height) where the predicate is true, if any.
    fn lowest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns the highest entry (by block height) where the predicate is true, if any.
    fn highest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool;
    /// Returns a range, including vacancies.
    fn range(&self, lbound: Option<u64>, ubound: Option<u64>) -> Vec<BlockChainEntry>;
}
