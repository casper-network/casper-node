//! A reference pool for items/objects.
//!
//! Its core responsibility is to deduplicate potentially expensive loads by keeping a weak
//! reference to any loaded object around, so that any load request for an object that is currently
//! in active use can be satisfied using the already existing copy.
//!
//! It differs from a cache in that it does not hold strong references to an item itself -- once an
//! item is no longer used, it will not be kept in the pool for a later request. As a consequence
//! the memory pool will never consume significantly more memory than what would otherwise be
//! required by the loaded objects that are in active use anyway and thus has an "infinite"
//! capacity.
use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Weak},
};

use datasize::DataSize;

/// A pool of items/objects.
///
/// Maintains a pool of weak references and automatically purges them in configurable intervals.
#[derive(DataSize, Debug)]
pub(super) struct ObjectPool<I> {
    /// The actual object pool.
    #[data_size(skip)]
    items: HashMap<I, Weak<Vec<u8>>>,
    /// Interval for garbage collection, will remove dead references on every n-th `put()`.
    garbage_collect_interval: u16,
    /// Counts how many objects have been added since the last garbage collect interval.
    put_count: u16,
}

impl<I> ObjectPool<I> {
    /// Creates a new object pool.
    pub(super) fn new(garbage_collect_interval: u16) -> Self {
        Self {
            items: HashMap::new(),
            garbage_collect_interval,
            put_count: 0,
        }
    }
}

impl<I> ObjectPool<I>
where
    I: Hash + Eq,
{
    /// Stores a serialized object in the pool.
    ///
    /// At configurable intervals (see `garbage_collect_interval`), the entire pool will be checked
    /// and dead references pruned.
    pub(super) fn put(&mut self, id: I, item: Weak<Vec<u8>>) {
        self.items.insert(id, item);

        if self.put_count >= self.garbage_collect_interval {
            self.items.retain(|_, item| item.strong_count() > 0);

            self.put_count = 0;
        }

        self.put_count += 1;
    }

    /// Retrieves an object from the pool, if present.
    pub(super) fn get(&self, id: &I) -> Option<Arc<Vec<u8>>> {
        self.items.get(id).and_then(Weak::upgrade)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use datasize::DataSize;

    use super::ObjectPool;
    use crate::types::{Deploy, Item};

    impl<I> ObjectPool<I>
    where
        I: DataSize,
    {
        fn num_entries(&self) -> usize {
            self.items.len()
        }
    }

    #[test]
    fn can_load_and_store_items() {
        let mut pool: ObjectPool<<Deploy as Item>::Id> = ObjectPool::new(5);
        let mut rng = crate::new_rng();

        let d1 = Deploy::random(&mut rng);
        let d2 = Deploy::random(&mut rng);
        let d1_id = *d1.id();
        let d2_id = *d2.id();
        let d1_serialized = bincode::serialize(&d1).expect("could not serialize first deploy");
        let d2_serialized = bincode::serialize(&d2).expect("could not serialize second deploy");

        let d1_shared = Arc::new(d1_serialized);
        let d2_shared = Arc::new(d2_serialized);

        assert!(pool.get(&d1_id).is_none());
        assert!(pool.get(&d2_id).is_none());

        pool.put(d1_id, Arc::downgrade(&d1_shared));
        assert!(Arc::ptr_eq(
            &pool.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));
        assert!(pool.get(&d2_id).is_none());

        pool.put(d2_id, Arc::downgrade(&d2_shared));
        assert!(Arc::ptr_eq(
            &pool.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));
        assert!(Arc::ptr_eq(
            &pool.get(&d2_id).expect("did not find d1"),
            &d2_shared
        ));
    }

    #[test]
    fn frees_memory_after_reference_loss() {
        let mut pool: ObjectPool<<Deploy as Item>::Id> = ObjectPool::new(5);
        let mut rng = crate::new_rng();

        let d1 = Deploy::random(&mut rng);
        let d1_id = *d1.id();
        let d1_serialized = bincode::serialize(&d1).expect("could not serialize first deploy");

        let d1_shared = Arc::new(d1_serialized);

        assert!(pool.get(&d1_id).is_none());

        pool.put(d1_id, Arc::downgrade(&d1_shared));
        assert!(Arc::ptr_eq(
            &pool.get(&d1_id).expect("did not find d1"),
            &d1_shared
        ));

        drop(d1_shared);
        assert!(pool.get(&d1_id).is_none());
    }

    #[test]
    fn garbage_is_collected() {
        let mut pool: ObjectPool<<Deploy as Item>::Id> = ObjectPool::new(5);
        let mut rng = crate::new_rng();

        assert_eq!(pool.num_entries(), 0);

        for i in 0..5 {
            let deploy = Deploy::random(&mut rng);
            let id = *deploy.id();
            let serialized = bincode::serialize(&deploy).expect("could not serialize first deploy");
            let shared = Arc::new(serialized);
            pool.put(id, Arc::downgrade(&shared));
            assert_eq!(pool.num_entries(), i + 1);
            drop(shared);
            assert_eq!(pool.num_entries(), i + 1);
        }

        let deploy = Deploy::random(&mut rng);
        let id = *deploy.id();
        let serialized = bincode::serialize(&deploy).expect("could not serialize first deploy");
        let shared = Arc::new(serialized);
        pool.put(id, Arc::downgrade(&shared));
        assert_eq!(pool.num_entries(), 1);
        drop(shared);
        assert_eq!(pool.num_entries(), 1);
    }
}
