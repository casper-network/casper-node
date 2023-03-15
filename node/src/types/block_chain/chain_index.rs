use crate::types::{
    block_chain::{indexed_block_chain::IndexedBlockChain, BlockChainEntry, Error},
    BlockHash, BlockHeader, DataSize,
};
use casper_types::EraId;
use itertools::Itertools;
use std::collections::{btree_map::Entry, hash_map::Entry as HashEntry, BTreeMap, HashMap};

/// Block index by height and reverse lookup (where known) by hash.
#[derive(DataSize, Debug, Default)]
pub(crate) struct ChainIndex {
    chain: BTreeMap<u64, BlockChainEntry>,
    index: HashMap<BlockHash, u64>,
}

impl ChainIndex {
    /// Are the chain and by height index both empty?
    pub(crate) fn is_empty(&self) -> bool {
        self.chain.is_empty() && self.index.is_empty()
    }

    fn register(&mut self, item: BlockChainEntry) {
        // don't waste mem on actual vacant entries
        if item.is_vacant() {
            return;
        }
        let block_height = item.block_height();
        // maintain the reverse lookup by block_hash where able
        if let Some(block_hash) = item.block_hash() {
            match self.index.entry(block_hash) {
                HashEntry::Occupied(entry) => {
                    let val = entry.get();
                    debug_assert!(
                        val.eq(&block_height),
                        "BlockChain: register existing block_height {} should match {}",
                        val,
                        block_height
                    );
                }
                HashEntry::Vacant(vacant) => {
                    vacant.insert(block_height);
                }
            }
        }
        // maintain the chain representation overlay
        match self.chain.entry(block_height) {
            Entry::Vacant(vacant) => {
                vacant.insert(item);
            }
            Entry::Occupied(mut entry) => {
                let val = entry.get_mut();
                *val = item;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn register_complete_from_parts(
        &mut self,
        block_height: u64,
        block_hash: BlockHash,
        era_id: EraId,
        is_switch_block: bool,
    ) {
        let entry = BlockChainEntry::Complete {
            block_height,
            block_hash,
            era_id,
            is_switch_block,
        };
        self.register(entry);
    }
}

impl IndexedBlockChain for ChainIndex {
    fn remove(&mut self, block_height: u64) -> Option<BlockChainEntry> {
        if let Some(entry) = self.chain.remove(&block_height) {
            if let Some(block_hash) = entry.block_hash() {
                let _ = self.index.remove(&block_hash);
            }
            Some(entry)
        } else {
            None
        }
    }

    fn register_proposed(&mut self, block_height: u64) -> Result<(), Error> {
        if let Some(highest) = self.highest(BlockChainEntry::all_non_vacant) {
            if false == highest.is_proposed() {
                return Err(Error::AttemptToProposeAboveTail);
            }
        }
        self.register(BlockChainEntry::new_proposed(block_height));
        Ok(())
    }

    fn register_finalized(&mut self, block_height: u64, era_id: EraId) -> Result<(), Error> {
        if self
            .higher_by_status(BlockChainEntry::all_non_vacant)
            .is_some()
        {
            return Err(Error::AttemptToFinalizeAboveTail);
        }
        self.register(BlockChainEntry::new_finalized(block_height, era_id));
        Ok(())
    }

    fn register_incomplete(&mut self, block_height: u64, block_hash: BlockHash, era_id: EraId) {
        self.register(BlockChainEntry::new_incomplete(
            block_height,
            block_hash,
            era_id,
        ));
    }

    fn register_complete(&mut self, block_header: &BlockHeader) {
        self.register(BlockChainEntry::new_complete(block_header));
    }

    fn by_height(&self, block_height: u64) -> BlockChainEntry {
        *self
            .chain
            .get(&block_height)
            .unwrap_or(&BlockChainEntry::vacant(block_height))
    }

    fn by_hash(&self, block_hash: &BlockHash) -> Option<BlockChainEntry> {
        if let Some(height) = self.index.get(block_hash) {
            return Some(self.by_height(*height));
        }
        None
    }

    fn by_parent(&self, parent_block_hash: &BlockHash) -> Option<BlockChainEntry> {
        if let Some(height) = self.index.get(parent_block_hash) {
            return Some(self.by_height(height + 1));
        }
        None
    }

    fn by_child(&self, child_block_hash: &BlockHash) -> Option<BlockChainEntry> {
        if let Some(height) = self.index.get(child_block_hash) {
            if height.eq(&0) {
                // genesis cannot have a parent
                return None;
            }
            return Some(self.by_height(height.saturating_sub(1)));
        }
        None
    }

    fn switch_block_by_era(&self, era_id: EraId) -> Option<BlockChainEntry> {
        self.chain
            .values()
            .find_or_first(|x| x.is_switch_block().unwrap_or(false) && x.era_id() == Some(era_id))
            .cloned()
    }

    fn is_incomplete(&self, block_height: u64) -> bool {
        self.chain
            .get(&block_height)
            .map(|b| b.is_incomplete())
            .unwrap_or(false)
    }

    fn is_complete(&self, block_height: u64) -> bool {
        self.chain
            .get(&block_height)
            .map(|b| b.is_complete())
            .unwrap_or(false)
    }

    fn is_switch_block(&self, block_height: u64) -> Option<bool> {
        self.chain
            .get(&block_height)
            .map(|b| b.is_switch_block())
            .unwrap_or(None)
    }

    fn is_immediate_switch_block(&self, block_height: u64) -> Option<bool> {
        if let Some(switch) = self.is_switch_block(block_height) {
            if block_height == 0 {
                // on legacy chains, first block is not a switch block,
                // on post 1.5 chains it is, and is considered to be an
                // immediate switch block though it has no parent as
                // it is the head of the chain
                return Some(true);
            }
            return if switch {
                // immediate switch block == block @ height is switch && parent is also switch
                self.is_switch_block(block_height.saturating_sub(1))
            } else {
                Some(false)
            };
        }
        None
    }

    fn lowest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.chain
            .values()
            .filter(|x| predicate(x))
            .min_by(|x, y| x.block_height().cmp(&y.block_height()))
            .cloned()
    }

    fn highest<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.chain
            .values()
            .filter(|x| predicate(x))
            .max_by(|x, y| x.block_height().cmp(&y.block_height()))
            .cloned()
    }

    fn higher_by_status<F>(&self, predicate: F) -> Option<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.chain
            .values()
            .filter(|x| predicate(x))
            .max_by(|x, y| x.higher_height_and_status(y))
            .cloned()
    }

    fn lowest_switch_block(&self) -> Option<BlockChainEntry> {
        self.chain
            .values()
            .filter(|x| x.is_complete() && x.is_switch_block().unwrap_or(false))
            .min_by(|x, y| x.block_height().cmp(&y.block_height()))
            .cloned()
    }

    fn highest_switch_block(&self) -> Option<BlockChainEntry> {
        self.chain
            .values()
            .filter(|x| x.is_complete() && x.is_switch_block().unwrap_or(false))
            .max_by(|x, y| x.block_height().cmp(&y.block_height()))
            .cloned()
    }

    fn all_by<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        self.chain
            .values()
            .filter(|x| predicate(x))
            .cloned()
            .collect_vec()
    }

    fn lowest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        match self
            .chain
            .values()
            .filter(|x| predicate(x))
            .min_by(|x, y| x.block_height().cmp(&y.block_height()))
        {
            None => {
                vec![]
            }
            Some(entry) => {
                let mut ret = vec![*entry];
                let mut idx = entry.block_height() + 1;
                loop {
                    let item = self.by_height(idx);
                    if predicate(&item) {
                        ret.push(item);
                        idx += 1;
                    } else {
                        break;
                    }
                }
                ret
            }
        }
    }

    fn highest_sequence<F>(&self, predicate: F) -> Vec<BlockChainEntry>
    where
        F: Fn(&BlockChainEntry) -> bool,
    {
        match self
            .chain
            .values()
            .filter(|x| predicate(x))
            .max_by(|x, y| x.block_height().cmp(&y.block_height()))
        {
            None => {
                vec![]
            }
            Some(entry) => {
                let mut ret = vec![*entry];
                let mut idx = entry.block_height() - 1;
                loop {
                    let item = self.by_height(idx);
                    if predicate(&item) {
                        ret.push(item);
                        idx -= 1;
                    } else {
                        break;
                    }
                }
                ret
            }
        }
    }

    fn range(&self, lbound: Option<u64>, ubound: Option<u64>) -> Vec<BlockChainEntry> {
        let mut ret = vec![];
        if self.chain.is_empty() {
            return ret;
        }
        let low = lbound.unwrap_or(0);
        let hi = ubound.unwrap_or(*self.chain.keys().max().unwrap_or(&0));
        if low > hi {
            return ret;
        }
        for height in low..=hi {
            match self.chain.get(&height) {
                None => {
                    ret.push(BlockChainEntry::vacant(height));
                }
                Some(entry) => ret.push(*entry),
            }
        }
        ret
    }
}
