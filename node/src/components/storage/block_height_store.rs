// use serde::{Deserialize, Serialize};

use super::Result;

/// Trait defining the API for a block height store managed by the storage component.
pub trait BlockHeightStore<H>: Send + Sync {
    /// Returns true if no entry previously existed at the given height.
    fn put(&self, height: u64, block_hash: H) -> Result<bool>;
    fn get(&self, height: u64) -> Result<Option<H>>;
    fn highest(&self) -> Result<Option<H>>;
}

#[cfg(test)]
mod tests {
    use std::cmp;

    use rand::Rng;

    use super::{
        super::{Config, InMemBlockHeightStore, LmdbBlockHeightStore},
        *,
    };
    use crate::testing::TestRng;

    fn should_put_then_get<T: BlockHeightStore<String>>(block_height_store: &mut T) {
        let mut rng = TestRng::new();

        let height = rng.gen();

        block_height_store.put(height, height.to_string()).unwrap();
        let maybe_hash = block_height_store.get(height).unwrap();
        let recovered_hash = maybe_hash.unwrap();

        assert_eq!(height.to_string(), recovered_hash);
    }

    #[test]
    fn lmdb_block_height_store_should_put_then_get() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_height_store =
            LmdbBlockHeightStore::new(config.path(), config.max_block_height_store_size()).unwrap();
        should_put_then_get(&mut lmdb_block_height_store);
    }

    #[test]
    fn in_mem_block_height_store_should_put_then_get() {
        let mut in_mem_block_height_store = InMemBlockHeightStore::new();
        should_put_then_get(&mut in_mem_block_height_store);
    }

    fn should_fail_get<T: BlockHeightStore<String>>(block_height_store: &mut T) {
        let mut rng = TestRng::new();

        let height = rng.gen();

        block_height_store.put(height, height.to_string()).unwrap();
        assert!(block_height_store
            .get(height.wrapping_add(1))
            .unwrap()
            .is_none());
    }

    #[test]
    fn lmdb_block_height_store_should_fail_to_get_unknown_version() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_height_store =
            LmdbBlockHeightStore::new(config.path(), config.max_block_height_store_size()).unwrap();
        should_fail_get(&mut lmdb_block_height_store);
    }

    #[test]
    fn in_mem_block_height_store_should_fail_to_get_unknown_version() {
        let mut in_mem_block_height_store = InMemBlockHeightStore::new();
        should_fail_get(&mut in_mem_block_height_store);
    }

    fn should_get_highest<T: BlockHeightStore<String>>(block_height_store: &mut T) {
        const BLOCK_COUNT: usize = 1000;
        let mut rng = TestRng::new();

        assert!(block_height_store.highest().unwrap().is_none());

        let mut max = 0;
        for _ in 0..BLOCK_COUNT {
            let height = rng.gen();
            max = cmp::max(max, height);

            block_height_store.put(height, height.to_string()).unwrap();
            let maybe_hash = block_height_store.highest().unwrap();
            let highest_hash = maybe_hash.unwrap();

            assert_eq!(max.to_string(), highest_hash);
        }
    }

    #[test]
    fn lmdb_block_height_store_should_get_highest() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_block_height_store =
            LmdbBlockHeightStore::new(config.path(), config.max_block_height_store_size()).unwrap();
        should_get_highest(&mut lmdb_block_height_store);
    }

    #[test]
    fn in_mem_block_height_store_should_get_highest() {
        let mut in_mem_block_height_store = InMemBlockHeightStore::new();
        should_get_highest(&mut in_mem_block_height_store);
    }

    #[test]
    fn lmdb_block_height_store_initialize_highest() {
        const BLOCK_COUNT: usize = 100;

        let (config, _tempdir) = Config::default_for_tests();
        let mut rng = TestRng::new();

        // Populate the DB then drop it.
        let max_height = {
            let lmdb_block_height_store =
                LmdbBlockHeightStore::new(config.path(), config.max_block_height_store_size())
                    .unwrap();

            let mut max = 0;
            for _ in 0..BLOCK_COUNT {
                let height = rng.gen();
                max = cmp::max(max, height);

                lmdb_block_height_store
                    .put(height, height.to_string())
                    .unwrap();
            }

            let maybe_hash: Option<String> = lmdb_block_height_store.highest().unwrap();
            let highest_hash = maybe_hash.unwrap();
            assert_eq!(max.to_string(), highest_hash);
            max
        };

        // Check a new DB correctly retrieves the max height.
        let lmdb_block_height_store =
            LmdbBlockHeightStore::new(config.path(), config.max_block_height_store_size()).unwrap();

        let maybe_hash: Option<String> = lmdb_block_height_store.highest().unwrap();
        let highest_hash = maybe_hash.unwrap();
        assert_eq!(max_height.to_string(), highest_hash);

        // Check adding a new higher value correctly updates the highest.
        let new_high = max_height + 1;
        lmdb_block_height_store
            .put(new_high, new_high.to_string())
            .unwrap();

        let maybe_hash: Option<String> = lmdb_block_height_store.highest().unwrap();
        let highest_hash = maybe_hash.unwrap();
        assert_eq!(new_high.to_string(), highest_hash);
    }
}
