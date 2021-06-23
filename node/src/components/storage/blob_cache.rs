//! A reference cache for items.

use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Weak},
};

use datasize::DataSize;

/// A cache of serialized items.
///
/// Maintains a collection of weak references and automatically purges them in configurable
/// intervals.
#[derive(DataSize, Debug)]
pub(crate) struct BlobCache<I> {
    /// The actual blob cache.
    #[data_size(skip)]
    items: HashMap<I, Weak<Vec<u8>>>,
    /// Interval for garbage collection, will remove dead references on every n-th `put()`.
    garbage_collect_interval: u16,
    /// Counts how many items have been added since the last garbage collect interval.
    put_count: u16,
}

impl<I> BlobCache<I> {
    /// Creates a new cache.
    pub(crate) fn new(garbage_collect_interval: u16) -> Self {
        Self {
            items: HashMap::new(),
            garbage_collect_interval,
            put_count: 0,
        }
    }
}

impl<I> BlobCache<I>
where
    I: Hash + Eq,
{
    /// Stores a serialized item (blob) in the cache.
    ///
    /// At configurable intervals (see `garbage_collect_interval`), the entire cache will be checked
    /// and dead references pruned.
    pub(crate) fn put(&mut self, id: I, item: Weak<Vec<u8>>) {
        self.items.insert(id, item);

        if self.put_count >= self.garbage_collect_interval {
            self.items.retain(|_, item| item.strong_count() > 0);

            self.put_count = 0;
        }

        self.put_count += 1;
    }

    /// Retrieves a blob from the cache, if present.
    pub(crate) fn get(&self, id: &I) -> Option<Arc<Vec<u8>>> {
        self.items.get(&id).and_then(Weak::upgrade)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use datasize::DataSize;

    use super::BlobCache;
    use crate::types::{Deploy, Item};

    impl<I> BlobCache<I>
    where
        I: DataSize,
    {
        fn num_entries(&self) -> usize {
            self.items.len()
        }
    }

    #[test]
    fn can_load_and_store_items() {
        let mut cache: BlobCache<<Deploy as Item>::Id> = BlobCache::new(5);
        let mut rng = crate::new_rng();

        let d1 = Deploy::random(&mut rng);
        let d2 = Deploy::random(&mut rng);
        let d1_id = *d1.id();
        let d2_id = *d2.id();
        let d1_serialized = bincode::serialize(&d1).expect("could not serialize first deploy");
        let d2_serialized = bincode::serialize(&d2).expect("could not serialize second deploy");

        let d1_shared = Arc::new(d1_serialized);
        let d2_shared = Arc::new(d2_serialized);

        assert!(cache.get(&d1_id).is_none());
        assert!(cache.get(&d2_id).is_none());

        cache.put(d1_id, Arc::downgrade(&d1_shared));
        assert!(Arc::ptr_eq(
            &cache.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));
        assert!(cache.get(&d2_id).is_none());

        cache.put(d2_id, Arc::downgrade(&d2_shared));
        assert!(Arc::ptr_eq(
            &cache.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));
        assert!(Arc::ptr_eq(
            &cache.get(&d2_id).expect("did not find d1"),
            &d2_shared
        ));
    }

    #[test]
    fn frees_memory_after_reference_loss() {
        let mut cache: BlobCache<<Deploy as Item>::Id> = BlobCache::new(5);
        let mut rng = crate::new_rng();

        let d1 = Deploy::random(&mut rng);
        let d1_id = *d1.id();
        let d1_serialized = bincode::serialize(&d1).expect("could not serialize first deploy");

        let d1_shared = Arc::new(d1_serialized);

        assert!(cache.get(&d1_id).is_none());

        cache.put(d1_id, Arc::downgrade(&d1_shared));
        assert!(Arc::ptr_eq(
            &cache.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));

        drop(d1_shared);
        assert!(cache.get(&d1_id).is_none());
    }

    #[test]
    fn garbage_is_collected() {
        let mut cache: BlobCache<<Deploy as Item>::Id> = BlobCache::new(5);
        let mut rng = crate::new_rng();

        assert_eq!(cache.num_entries(), 0);

        for i in 0..5 {
            let deploy = Deploy::random(&mut rng);
            let id = *deploy.id();
            let serialized = bincode::serialize(&deploy).expect("could not serialize first deploy");
            let shared = Arc::new(serialized);
            cache.put(id, Arc::downgrade(&shared));
            assert_eq!(cache.num_entries(), i + 1);
            drop(shared);
            assert_eq!(cache.num_entries(), i + 1);
        }

        let deploy = Deploy::random(&mut rng);
        let id = *deploy.id();
        let serialized = bincode::serialize(&deploy).expect("could not serialize first deploy");
        let shared = Arc::new(serialized);
        cache.put(id, Arc::downgrade(&shared));
        assert_eq!(cache.num_entries(), 1);
        drop(shared);
        assert_eq!(cache.num_entries(), 1);
    }
}
