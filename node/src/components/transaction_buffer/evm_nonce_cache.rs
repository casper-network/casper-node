//! A cache of EVM account nonces in the state of the highest executed block.

use std::collections::{BTreeMap, HashMap};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{evm, Digest};

/// The default number of accounts whose nonces the cache holds.
pub(crate) const DEFAULT_EVM_NONCE_CACHE_CAPACITY: usize = 1000;

/// Which entry the cache evicts when it is full.
#[derive(Copy, Clone, Debug, Default, DataSize, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvmNonceCacheEvictionPolicy {
    /// Evict the entry read least recently.
    #[default]
    LeastRecentlyRead,
}

/// The executed block whose state the cached nonces are from.
#[derive(Copy, Clone, Debug, DataSize, Eq, PartialEq)]
pub(crate) struct ExecutedTip {
    pub(crate) height: u64,
    pub(crate) state_root_hash: Digest,
}

/// The nonces of EVM accounts in the state of the highest executed block this node knows of.
///
/// Every entry is from the state of `tip`. Executing a block moves the tip forward and overwrites
/// the entries of the accounts whose nonces the block wrote; if the cache can't be sure it saw all
/// of a block's nonce writes (a gap in the executed blocks, or a block without execution results),
/// it drops every entry. Accounts missing from the cache are read from global state at the tip.
#[derive(Debug, DataSize)]
pub(crate) struct EvmNonceCache {
    capacity: usize,
    eviction_policy: EvmNonceCacheEvictionPolicy,
    tip: Option<ExecutedTip>,
    /// Nonce and the tick of the last read or write, by account.
    #[data_size(skip)]
    entries: HashMap<evm::Address, (u64, u64)>,
    /// Account by the tick of its last read or write, oldest first.
    #[data_size(skip)]
    by_tick: BTreeMap<u64, evm::Address>,
    next_tick: u64,
}

impl EvmNonceCache {
    pub(crate) fn new(capacity: usize, eviction_policy: EvmNonceCacheEvictionPolicy) -> Self {
        EvmNonceCache {
            capacity,
            eviction_policy,
            tip: None,
            entries: HashMap::new(),
            by_tick: BTreeMap::new(),
            next_tick: 0,
        }
    }

    /// The executed block the cached nonces are from, if any.
    pub(crate) fn tip(&self) -> Option<ExecutedTip> {
        self.tip
    }

    /// Returns the cached nonce of `account`, marking it as read.
    pub(crate) fn get(&mut self, account: &evm::Address) -> Option<u64> {
        let tick = self.tick();
        let (nonce, last_tick) = self.entries.get_mut(account)?;
        let _ = self.by_tick.remove(last_tick);
        *last_tick = tick;
        let _ = self.by_tick.insert(tick, *account);
        Some(*nonce)
    }

    /// Returns the accounts among `accounts` whose nonces are not cached.
    pub(crate) fn missing<'a>(
        &self,
        accounts: impl IntoIterator<Item = &'a evm::Address>,
    ) -> Vec<evm::Address> {
        let mut missing: Vec<_> = accounts
            .into_iter()
            .filter(|account| !self.entries.contains_key(account))
            .copied()
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// Caches nonces read from global state at `tip`.
    ///
    /// They are dropped if the cache has moved on to another executed block in the meantime.
    /// Returns whether they were cached.
    pub(crate) fn insert_fetched(
        &mut self,
        tip: ExecutedTip,
        nonces: impl IntoIterator<Item = (evm::Address, u64)>,
    ) -> bool {
        if self.tip != Some(tip) {
            return false;
        }
        for (account, nonce) in nonces {
            self.insert(account, nonce);
        }
        true
    }

    /// Moves the cache to the state after executing a block.
    ///
    /// `nonce_writes` are the nonces the block wrote, or `None` if they are unknown, in which case
    /// every entry is dropped. Entries are also dropped if the block does not directly follow the
    /// current tip, since the nonce writes of the blocks in between were not seen.
    pub(crate) fn apply_executed_block(
        &mut self,
        tip: ExecutedTip,
        nonce_writes: Option<&BTreeMap<evm::Address, u64>>,
    ) {
        match self.tip {
            Some(current) if tip.height <= current.height => {
                // An older or repeated block; the cache is already past it.
                return;
            }
            Some(current) if tip.height == current.height + 1 => {}
            Some(_) | None => self.clear(),
        }
        self.tip = Some(tip);
        match nonce_writes {
            Some(nonce_writes) => {
                for (account, nonce) in nonce_writes {
                    self.insert(*account, *nonce);
                }
            }
            None => self.clear(),
        }
    }

    /// Sets the executed block the cached nonces are from, dropping every entry if it changes.
    pub(crate) fn reset_tip(&mut self, tip: ExecutedTip) {
        if self.tip != Some(tip) {
            self.clear();
            self.tip = Some(tip);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn insert(&mut self, account: evm::Address, nonce: u64) {
        if self.capacity == 0 {
            return;
        }
        let tick = self.tick();
        if let Some((_, last_tick)) = self.entries.insert(account, (nonce, tick)) {
            let _ = self.by_tick.remove(&last_tick);
        }
        let _ = self.by_tick.insert(tick, account);
        while self.entries.len() > self.capacity {
            match self.eviction_policy {
                EvmNonceCacheEvictionPolicy::LeastRecentlyRead => {
                    let Some((_, evicted)) = self.by_tick.pop_first() else {
                        break;
                    };
                    let _ = self.entries.remove(&evicted);
                }
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.by_tick.clear();
    }

    fn tick(&mut self) -> u64 {
        let tick = self.next_tick;
        self.next_tick += 1;
        tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(byte: u8) -> evm::Address {
        evm::Address::new([byte; 20])
    }

    fn tip(height: u64) -> ExecutedTip {
        ExecutedTip {
            height,
            state_root_hash: Digest::hash(height.to_le_bytes()),
        }
    }

    #[test]
    fn evicts_least_recently_read_entry() {
        let mut cache = EvmNonceCache::new(2, EvmNonceCacheEvictionPolicy::LeastRecentlyRead);
        cache.reset_tip(tip(1));
        assert!(cache.insert_fetched(tip(1), [(address(1), 1), (address(2), 2)]));
        // Reading 1 makes 2 the least recently read.
        assert_eq!(cache.get(&address(1)), Some(1));
        assert!(cache.insert_fetched(tip(1), [(address(3), 3)]));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&address(2)), None);
        assert_eq!(cache.get(&address(1)), Some(1));
        assert_eq!(cache.get(&address(3)), Some(3));
    }

    #[test]
    fn drops_nonces_fetched_at_a_stale_tip() {
        let mut cache = EvmNonceCache::new(10, EvmNonceCacheEvictionPolicy::LeastRecentlyRead);
        cache.reset_tip(tip(1));
        cache.apply_executed_block(tip(2), Some(&BTreeMap::new()));
        assert!(!cache.insert_fetched(tip(1), [(address(1), 1)]));
        assert_eq!(cache.get(&address(1)), None);
    }

    #[test]
    fn executed_blocks_update_entries() {
        let mut cache = EvmNonceCache::new(10, EvmNonceCacheEvictionPolicy::LeastRecentlyRead);
        cache.reset_tip(tip(1));
        assert!(cache.insert_fetched(tip(1), [(address(1), 1), (address(2), 5)]));
        cache.apply_executed_block(tip(2), Some(&BTreeMap::from([(address(1), 2)])));
        assert_eq!(cache.tip(), Some(tip(2)));
        assert_eq!(cache.get(&address(1)), Some(2));
        assert_eq!(cache.get(&address(2)), Some(5));
    }

    #[test]
    fn gap_or_unknown_writes_clear_entries() {
        let mut cache = EvmNonceCache::new(10, EvmNonceCacheEvictionPolicy::LeastRecentlyRead);
        cache.reset_tip(tip(1));
        assert!(cache.insert_fetched(tip(1), [(address(1), 1)]));
        cache.apply_executed_block(tip(3), Some(&BTreeMap::new()));
        assert_eq!(cache.get(&address(1)), None);

        assert!(cache.insert_fetched(tip(3), [(address(1), 1)]));
        cache.apply_executed_block(tip(4), None);
        assert_eq!(cache.get(&address(1)), None);
        assert_eq!(cache.tip(), Some(tip(4)));

        // An older block doesn't move the cache back.
        assert!(cache.insert_fetched(tip(4), [(address(1), 9)]));
        cache.apply_executed_block(tip(2), None);
        assert_eq!(cache.get(&address(1)), Some(9));
    }
}
