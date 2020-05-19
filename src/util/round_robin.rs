//! Round-robin scheduling.
//!
//! This module implements a weighted round-robin scheduler that ensures no deadlocks occur, but
//! still allows prioriting events from one source over another. The module uses `tokio`s
//! synchronization primitives under the hood.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::num::NonZeroUsize;
use tokio::sync;

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
    state: sync::Mutex<IterationState<K>>,

    /// A list of slots that are round-robin'd.
    slots: Vec<Slot<K>>,

    /// Actual queues.
    queues: HashMap<K, sync::Mutex<VecDeque<I>>>,

    /// Number of items in all queues combined.
    total: sync::Semaphore,
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
/// A slot marks the scheduling position, i.e. which queue we are currently
/// polling and how many tickets it has left before the next one is due.
#[derive(Copy, Clone, Debug)]
struct Slot<K> {
    /// The key, identifying a queue.
    key: K,

    /// Number of items to return before moving on to the next queue.
    tickets: usize,
}

impl<I, K> WeightedRoundRobin<I, K>
where
    K: Copy + Clone + Eq + Hash,
{
    /// Create new weighted round-robin scheduler.
    ///
    /// Creates a queue for each pair given in `weights`. The second component
    /// of each `weight` is the number of times to return items from one
    /// queue before moving on to the next one.
    pub fn new(weights: Vec<(K, NonZeroUsize)>) -> Self {
        assert!(!weights.is_empty(), "must provide at least one slot");

        let queues = weights
            .iter()
            .map(|(idx, _)| (*idx, sync::Mutex::new(VecDeque::new())))
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
            state: sync::Mutex::new(IterationState {
                active_slot,
                active_slot_idx: 0,
            }),
            slots,
            queues,
            total: sync::Semaphore::new(0),
        }
    }

    /// Push an item to a queue identified by key.
    ///
    /// ## Panics
    ///
    /// Panics if the queue identified by key `queue` does not exist.
    pub async fn push(&self, item: I, queue: K) {
        self.queues
            .get(&queue)
            .expect("tried to push to non-existant queue")
            .lock()
            .await
            .push_back(item);

        // We increase the item count after we've put the item into the queue.
        self.total.add_permits(1);
    }

    /// Return the next item from queue.
    ///
    /// Returns `None` if the queue is empty or an internal error occurred. The
    /// latter should never happen.
    pub async fn pop(&self) -> (I, K) {
        self.total.acquire().await.forget();

        let mut inner = self.state.lock().await;

        // We know we have at least one item in a queue.
        loop {
            let mut current_queue = self
                .queues
                // The queue disappearing should never happen.
                .get(&inner.active_slot.key)
                .expect("the queue disappeared. this should not happen")
                .lock()
                .await;

            if inner.active_slot.tickets == 0 || current_queue.is_empty() {
                // Go to next queue slot if we've exhausted the current queue.
                inner.active_slot_idx = (inner.active_slot_idx + 1) % self.slots.len();
                inner.active_slot = self.slots[inner.active_slot_idx];
                continue;
            }

            // We have hit a queue that is not empty. Decrease tickets and pop.
            inner.active_slot.tickets -= 1;

            break (
                current_queue
                    .pop_front()
                    // We hold the queue's lock and checked `is_empty` earlier.
                    .expect("item disappeared. this should not happen"),
                inner.active_slot.key,
            );
        }
    }
}
