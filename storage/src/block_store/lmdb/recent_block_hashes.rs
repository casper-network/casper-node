//! Bounded, height-indexed cache of committed block hashes.

use casper_types::BlockHash;

// Keep the stored tip and the 8,191 ancestors needed by EIP-2935, even if the
// block being executed has already been stored. Older lookups fall back to LMDB.
pub(super) const HISTORY_CACHE_SIZE: usize = (crate::eip2935::HISTORY_BUFFER_LENGTH + 1) as usize;

#[derive(Debug)]
pub(super) struct RecentBlockHashes {
    slots: Vec<Option<(u64, BlockHash)>>,
}

impl Default for RecentBlockHashes {
    fn default() -> Self {
        Self {
            slots: vec![None; HISTORY_CACHE_SIZE],
        }
    }
}

impl RecentBlockHashes {
    fn index(height: u64) -> usize {
        (height % HISTORY_CACHE_SIZE as u64) as usize
    }

    pub(super) fn get(&self, height: u64) -> Option<BlockHash> {
        match self.slots[Self::index(height)] {
            Some((cached_height, hash)) if cached_height == height => Some(hash),
            _ => None,
        }
    }

    pub(super) fn insert(&mut self, height: u64, hash: BlockHash) {
        let slot = &mut self.slots[Self::index(height)];
        // Sync may deliver older headers after newer ones. They can populate a
        // gap, but must not evict a newer height that uses the same slot.
        if matches!(slot, Some((cached_height, _)) if *cached_height > height) {
            return;
        }
        *slot = Some((height, hash));
    }

    pub(super) fn clear(&mut self) {
        self.slots.fill(None);
    }
}

#[cfg(test)]
mod tests {
    use casper_types::Digest;

    use super::*;

    fn hash(height: u64) -> BlockHash {
        BlockHash::new(Digest::hash(height.to_le_bytes()))
    }

    #[test]
    fn wraparound_and_out_of_order_inserts_keep_the_newer_height() {
        let mut cache = RecentBlockHashes::default();
        let newer = HISTORY_CACHE_SIZE as u64 + 7;
        cache.insert(7, hash(7));
        cache.insert(newer, hash(newer));
        assert_eq!(cache.get(7), None);
        assert_eq!(cache.get(newer), Some(hash(newer)));
        cache.insert(7, hash(7));
        assert_eq!(cache.get(newer), Some(hash(newer)));
        assert_eq!(cache.slots.len(), HISTORY_CACHE_SIZE);
    }

    #[test]
    fn missing_heights_and_large_heights_are_distinct() {
        let mut cache = RecentBlockHashes::default();
        cache.insert(0, hash(0));
        cache.insert(u64::MAX, hash(u64::MAX));
        assert_eq!(cache.get(0), Some(hash(0)));
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(u64::MAX), Some(hash(u64::MAX)));
        cache.clear();
        assert_eq!(cache.get(0), None);
        assert_eq!(cache.get(u64::MAX), None);
    }
}
