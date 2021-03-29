//! Weighted round-robin scheduling.
//!
//! This module implements a weighted round-robin scheduler that ensures no deadlocks occur, but
//! still allows prioritizing events from one source over another. The module uses `tokio`'s
//! synchronization primitives under the hood.

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    fs::File,
    hash::Hash,
    io::{self, BufWriter, Write},
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

use enum_iterator::IntoEnumIterator;
use serde::{ser::SerializeMap, Serialize, Serializer};
use tokio::sync::{Mutex, MutexGuard, Semaphore};

/// Weighted round-robin scheduler.
///
/// The weighted round-robin scheduler keeps queues internally and returns an item from a queue
/// when asked. Each queue is assigned a weight, which is simply the amount of items maximally
/// returned from it before moving on to the next queue.
///
/// If a queue is empty, it is skipped until the next round. Queues are processed in the order they
/// are passed to the constructor function.
///
/// The scheduler keeps track internally which queue needs to be popped next.
#[derive(Debug)]
pub struct WeightedRoundRobin<I, K> {
    /// Current iteration state.
    state: Mutex<IterationState<K>>,

    /// A list of slots that are round-robin'd.
    slots: Vec<Slot<K>>,

    /// Actual queues.
    queues: HashMap<K, QueueState<I>>,

    /// Number of items in all queues combined.
    total: Semaphore,
}

/// State that wraps queue and its event count.
#[derive(Debug)]
struct QueueState<I> {
    /// A queue's event counter.
    ///
    /// Do not modify this unless you are holding the `queue` lock.
    event_count: AtomicUsize,
    queue: Mutex<VecDeque<I>>,
}

impl<I> QueueState<I> {
    fn new() -> Self {
        QueueState {
            event_count: AtomicUsize::new(0),
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Remove all events from a queue.
    async fn drain(&self) -> Vec<I> {
        let mut guard = self.queue.lock().await;
        let events: Vec<I> = guard.drain(..).collect();
        self.event_count.fetch_sub(events.len(), Ordering::SeqCst);
        events
    }

    #[inline]
    async fn push_back(&self, element: I) {
        self.queue.lock().await.push_back(element);
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn dec_count(&self) {
        self.event_count.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    fn event_count(&self) -> usize {
        self.event_count.load(Ordering::SeqCst)
    }
}

/// The inner state of the queue iteration.
#[derive(Copy, Clone, Debug)]
struct IterationState<K> {
    /// The currently active slot.
    ///
    /// Once it has no tickets left, the next slot is loaded.
    active_slot: Slot<K>,

    /// The position of the active slot. Used to calculate the next slot.
    active_slot_idx: usize,
}

/// An internal slot in the round-robin scheduler.
///
/// A slot marks the scheduling position, i.e. which queue we are currently polling and how many
/// tickets it has left before the next one is due.
#[derive(Copy, Clone, Debug)]
struct Slot<K> {
    /// The key, identifying a queue.
    key: K,

    /// Number of items to return before moving on to the next queue.
    tickets: usize,
}

impl<I, K> WeightedRoundRobin<I, K>
where
    I: Serialize,
    K: Copy + Clone + Eq + Hash + IntoEnumIterator + Serialize,
{
    /// Create a snapshot of the queue by locking it and serializing it.
    ///
    /// The serialized events are streamed directly into `serializer`.
    ///
    /// # Warning
    ///
    /// This function locks all queues in the order defined by the order defined by
    /// `IntoEnumIterator`. Calling it multiple times in parallel is safe, but other code that locks
    /// more than one queue at the same time needs to be aware of this.
    pub async fn snapshot<S: Serializer>(&self, serializer: S) -> Result<(), S::Error> {
        // Lock all queues in order get a snapshot, but release eagerly. This way we are guaranteed
        // to have a consistent result, but we also allow for queues to be used again earlier.
        let mut locks = Vec::new();

        for kind in K::into_enum_iter() {
            let queue_guard = self
                .queues
                .get(&kind)
                .expect("missing queue while snapshotting")
                .queue
                .lock()
                .await;

            locks.push((kind, queue_guard));
        }

        let mut map = serializer.serialize_map(Some(locks.len()))?;

        // By iterating over the guards, they are dropped in order.
        for (kind, guard) in locks {
            let vd = &*guard;
            map.serialize_key(&kind)?;
            map.serialize_value(vd)?;
        }
        map.end()?;

        Ok(())
    }
}

impl<I, K> WeightedRoundRobin<I, K>
where
    I: Debug,
    K: Copy + Clone + Eq + Hash + IntoEnumIterator + Debug,
{
    /// Dump the contents of the queues (`Debug` representation) to a given file.
    pub async fn debug_dump(&self, file: &mut File) -> Result<(), io::Error> {
        let locks = self.lock_queues().await;

        let mut writer = BufWriter::new(file);
        for (kind, guard) in locks {
            let queue = &*guard;
            writer.write_all(format!("Queue: {:?} ({}) [\n", kind, queue.len()).as_bytes())?;
            for event in queue.iter() {
                writer.write_all(format!("\t{:?}\n", event).as_bytes())?;
            }
            writer.write_all(b"]\n")?;
        }
        writer.flush()
    }

    /// Lock all queues in a well-defined order to avoid deadlocks conditions.
    async fn lock_queues<'a>(&'a self) -> Vec<(K, MutexGuard<'a, VecDeque<I>>)> {
        let mut locks = Vec::new();
        for kind in K::into_enum_iter() {
            let queue_guard = self
                .queues
                .get(&kind)
                .expect("missing queue while locking")
                .queue
                .lock()
                .await;

            locks.push((kind, queue_guard));
        }

        locks
    }
}

impl<I, K> WeightedRoundRobin<I, K>
where
    K: Copy + Clone + Eq + Hash,
{
    /// Creates a new weighted round-robin scheduler.
    ///
    /// Creates a queue for each pair given in `weights`. The second component of each `weight` is
    /// the number of times to return items from one queue before moving on to the next one.
    pub(crate) fn new(weights: Vec<(K, NonZeroUsize)>) -> Self {
        assert!(!weights.is_empty(), "must provide at least one slot");

        let queues = weights
            .iter()
            .map(|(idx, _)| (*idx, QueueState::new()))
            .collect();
        let slots: Vec<Slot<K>> = weights
            .into_iter()
            .map(|(key, tickets)| Slot {
                key,
                tickets: tickets.get(),
            })
            .collect();
        let active_slot = slots[0];

        WeightedRoundRobin {
            state: Mutex::new(IterationState {
                active_slot,
                active_slot_idx: 0,
            }),
            slots,
            queues,
            total: Semaphore::new(0),
        }
    }

    /// Pushes an item to a queue identified by key.
    ///
    /// ## Panics
    ///
    /// Panics if the queue identified by key `queue` does not exist.
    pub(crate) async fn push(&self, item: I, queue: K) {
        self.queues
            .get(&queue)
            .expect("tried to push to non-existent queue")
            .push_back(item)
            .await;

        // We increase the item count after we've put the item into the queue.
        self.total.add_permits(1);
    }

    /// Returns the next item from queue.
    ///
    /// Asynchronously waits until a queue is non-empty or panics if an internal error occurred.
    pub(crate) async fn pop(&self) -> (I, K) {
        // Safe to `expect` here as the only way for acquiring a permit to fail would be if the
        // `self.total` semaphore were closed.
        self.total.acquire().await.expect("should acquire").forget();

        let mut inner = self.state.lock().await;

        // We know we have at least one item in a queue.
        loop {
            let queue_state = self
                .queues
                // The queue disappearing should never happen.
                .get(&inner.active_slot.key)
                .expect("the queue disappeared. this should not happen");

            let mut current_queue = queue_state.queue.lock().await;

            if inner.active_slot.tickets == 0 || current_queue.is_empty() {
                // Go to next queue slot if we've exhausted the current queue.
                inner.active_slot_idx = (inner.active_slot_idx + 1) % self.slots.len();
                inner.active_slot = self.slots[inner.active_slot_idx];
                continue;
            }

            // We have hit a queue that is not empty. Decrease tickets and pop.
            inner.active_slot.tickets -= 1;

            let item = current_queue
                .pop_front()
                // We hold the queue's lock and checked `is_empty` earlier.
                .expect("item disappeared. this should not happen");
            queue_state.dec_count();
            break (item, inner.active_slot.key);
        }
    }

    /// Drains all events from a specific queue.
    pub(crate) async fn drain_queue(&self, queue: K) -> Vec<I> {
        let events = self
            .queues
            .get(&queue)
            .expect("queue to be drained disappeared")
            .drain()
            .await;

        // TODO: This is racy if someone is calling `pop` at the same time.
        // TODO: Use `acquire_many` as soon as tokio has been upgraded.
        for _ in 0..events.len() {
            self.total.acquire().await.expect("should acquire").forget();
        }

        events
    }

    /// Returns the number of events currently in the queue.
    #[cfg(test)]
    pub(crate) fn item_count(&self) -> usize {
        self.total.available_permits()
    }

    /// Returns the number of events in each of the queues.
    pub(crate) fn event_queues_counts(&self) -> HashMap<K, usize> {
        self.queues
            .iter()
            .map(|(key, queue)| (*key, queue.event_count()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use futures::{future::FutureExt, join};

    use super::*;

    #[repr(usize)]
    #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
    enum QueueKind {
        One = 1,
        Two,
    }

    fn weights() -> Vec<(QueueKind, NonZeroUsize)> {
        unsafe {
            vec![
                (QueueKind::One, NonZeroUsize::new_unchecked(1)),
                (QueueKind::Two, NonZeroUsize::new_unchecked(2)),
            ]
        }
    }

    #[tokio::test]
    async fn should_respect_weighting() {
        let scheduler = WeightedRoundRobin::<char, QueueKind>::new(weights());
        // Push three items on to each queue
        let future1 = scheduler
            .push('a', QueueKind::One)
            .then(|_| scheduler.push('b', QueueKind::One))
            .then(|_| scheduler.push('c', QueueKind::One));
        let future2 = scheduler
            .push('d', QueueKind::Two)
            .then(|_| scheduler.push('e', QueueKind::Two))
            .then(|_| scheduler.push('f', QueueKind::Two));
        join!(future2, future1);

        // We should receive the popped values in the order a, d, e, b, f, c
        assert_eq!(('a', QueueKind::One), scheduler.pop().await);
        assert_eq!(('d', QueueKind::Two), scheduler.pop().await);
        assert_eq!(('e', QueueKind::Two), scheduler.pop().await);
        assert_eq!(('b', QueueKind::One), scheduler.pop().await);
        assert_eq!(('f', QueueKind::Two), scheduler.pop().await);
        assert_eq!(('c', QueueKind::One), scheduler.pop().await);
    }
}
