mod block_chain_entry;
mod chain_index;
mod error;
mod indexed_block_chain;
mod tests;

use datasize::DataSize;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub(crate) use block_chain_entry::BlockChainEntry;

use crate::types::BlockHash;
use casper_types::EraId;

use crate::types::{block_chain::indexed_block_chain::IndexedBlockChain, BlockHeader};
use chain_index::ChainIndex;
pub(crate) use error::Error;

const POISONED_LOCK: &str = "poisoned lock on chain index";

/// Chain-wise representation of blocks by height and (where known) hash.
#[derive(DataSize, Debug, Default)]
pub(crate) struct BlockChain {
    inner: Arc<RwLock<ChainIndex>>,
}

// todo!("remove allow unused once integrated");
#[allow(unused)]
impl BlockChain {
    /// Instantiate an instance of BlockChain.
    pub(crate) fn new() -> Self {
        BlockChain::default()
    }

    /// Is index empty?
    pub(crate) fn is_empty(&self) -> bool {
        self.read_inner().is_empty()
    }

    // get write guard
    fn write_inner(&self) -> RwLockWriteGuard<ChainIndex> {
        self.inner.write().expect(POISONED_LOCK)
    }

    // get read guard
    fn read_inner(&self) -> RwLockReadGuard<ChainIndex> {
        self.inner.read().expect(POISONED_LOCK)
    }

    #[cfg(test)]
    fn register_complete_from_parts(
        &mut self,
        block_height: u64,
        block_hash: BlockHash,
        era_id: EraId,
        is_switch_block: bool,
    ) {
        self.write_inner().register_complete_from_parts(
            block_height,
            block_hash,
            era_id,
            is_switch_block,
        )
    }
}

impl IndexedBlockChain for BlockChain {
    fn remove(&mut self, block_height: u64) -> Option<BlockChainEntry> {
        self.write_inner().remove(block_height)
    }

    fn register_proposed(&mut self, block_height: u64) -> Result<(), Error> {
        self.write_inner().register_proposed(block_height)
    }

    fn register_finalized(&mut self, block_height: u64, era_id: EraId) -> Result<(), Error> {
        self.write_inner().register_finalized(block_height, era_id)
    }

    fn register_incomplete(&mut self, block_height: u64, block_hash: BlockHash, era_id: EraId) {
        self.write_inner()
            .register_incomplete(block_height, block_hash, era_id)
    }

    fn register_complete(&mut self, block_header: &BlockHeader) {
        self.write_inner().register_complete(block_header)
    }

    fn by_height(&self, block_height: u64) -> BlockChainEntry {
        self.read_inner().by_height(block_height)
    }

    fn by_hash(&self, block_hash: &BlockHash) -> Option<BlockChainEntry> {
        self.read_inner().by_hash(block_hash)
    }

    fn by_parent(&self, parent_block_hash: &BlockHash) -> Option<BlockChainEntry> {
        self.read_inner().by_parent(parent_block_hash)
    }

    fn by_child(&self, child_block_hash: &BlockHash) -> Option<BlockChainEntry> {
        self.read_inner().by_child(child_block_hash)
    }

    fn switch_block_by_era(&self, era_id: EraId) -> Option<BlockChainEntry> {
        self.read_inner().switch_block_by_era(era_id)
    }

    fn is_incomplete(&self, block_height: u64) -> bool {
        self.read_inner().is_incomplete(block_height)
    }

    fn is_complete(&self, block_height: u64) -> bool {
        self.read_inner().is_complete(block_height)
    }

    fn is_switch_block(&self, block_height: u64) -> Option<bool> {
        self.read_inner().is_switch_block(block_height)
    }

    fn is_immediate_switch_block(&self, block_height: u64) -> Option<bool> {
        self.read_inner().is_immediate_switch_block(block_height)
    }

    fn lowest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().lowest(predicate)
    }

    fn highest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().highest(predicate)
    }

    fn higher_by_status<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().higher_by_status(predicate)
    }

    fn lowest_switch_block(&self) -> Option<BlockChainEntry> {
        self.read_inner().lowest_switch_block()
    }

    fn highest_switch_block(&self) -> Option<BlockChainEntry> {
        self.read_inner().highest_switch_block()
    }

    fn all_by<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().all_by(predicate)
    }

    fn lowest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().lowest_sequence(predicate)
    }

    fn highest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.read_inner().highest_sequence(predicate)
    }

    fn range(&self, lbound: Option<u64>, ubound: Option<u64>) -> Vec<BlockChainEntry> {
        self.read_inner().range(lbound, ubound)
    }
}
