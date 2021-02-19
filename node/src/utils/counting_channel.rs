//! Support for counting channels.
//!
//! Regular tokio channels do not make the number of pending items available. Counting channels wrap
//! regular tokio channels but keep a counter of the number of items up-to-date with every `send`
//! and `recv`. The `len` method can be used to retrieve the number of items.
//!
//! The channel also counts the heap memory used by items on the stack if `DataSize` is implemented
//! for `T` and `send_datasized` is used instead of send. Internally it stores the size of each item
//! instead of recalculating on `recv` to avoid underflows due to interior mutability.

use std::{
    mem::size_of,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use datasize::DataSize;
use tokio::sync::mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Create an unbounded tokio channel, wrapped in counting sender/receiver.
pub fn counting_unbounded_channel<T>() -> (CountingSender<T>, CountingReceiver<T>) {
    let (tx, rx) = unbounded_channel();
    let counter = Arc::new(AtomicUsize::new(0));
    let memory_used = Arc::new(AtomicUsize::new(0));

    (
        CountingSender {
            sender: tx,
            counter: counter.clone(),
            memory_used: memory_used.clone(),
        },
        CountingReceiver {
            receiver: rx,
            counter,
            memory_used,
        },
    )
}

/// Counting sender.
#[derive(Debug)]
pub struct CountingSender<T> {
    sender: UnboundedSender<(usize, T)>,
    counter: Arc<AtomicUsize>,
    memory_used: Arc<AtomicUsize>,
}

impl<T> CountingSender<T> {
    /// Internal sending function.
    #[inline]
    fn do_send(&self, size: usize, message: T) -> Result<usize, SendError<T>> {
        // We increase the counters before attempting to add values, to avoid a race that would
        // occur if a receiver fetches the item in between, which would cause a usize underflow.
        self.memory_used.fetch_add(size, Ordering::SeqCst);
        let count = self.counter.fetch_add(1, Ordering::SeqCst);

        self.sender
            .send((size, message))
            .map_err(|err| {
                // Item was rejected, correct counts.
                self.memory_used.fetch_sub(size, Ordering::SeqCst);
                self.counter.fetch_sub(1, Ordering::SeqCst);
                SendError(err.0 .1)
            })
            .map(|_| count)
    }

    /// Sends a message down the channel, increasing the count on success.
    #[inline]
    pub fn send(&self, message: T) -> Result<usize, SendError<T>> {
        self.do_send(0, message)
    }

    /// Returns the count, i.e. message currently inside the channel.
    #[inline]
    pub fn len(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }
}

impl<T> CountingSender<T>
where
    T: DataSize,
{
    /// Sends a message down the channel, recording heap memory usage and count.
    #[inline]
    pub fn send_datasized(&self, message: T) -> Result<usize, SendError<T>> {
        self.do_send(
            message.estimate_heap_size() + size_of::<(usize, T)>(),
            message,
        )
    }
}

impl<T> DataSize for CountingSender<T>
where
    T: DataSize,
{
    const IS_DYNAMIC: bool = T::IS_DYNAMIC;
    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        self.memory_used.load(Ordering::SeqCst)
    }
}

pub struct CountingReceiver<T> {
    receiver: UnboundedReceiver<(usize, T)>,
    counter: Arc<AtomicUsize>,
    memory_used: Arc<AtomicUsize>,
}

impl<T> CountingReceiver<T> {
    /// Receives a message from the channel, decreasing the count on success.
    #[inline]
    pub async fn recv(&mut self) -> Option<T> {
        self.receiver.recv().await.map(|(size, value)| {
            self.memory_used.fetch_sub(size, Ordering::SeqCst);
            self.counter.fetch_sub(1, Ordering::SeqCst);
            value
        })
    }

    /// Returns the count, i.e. message currently inside the channel.
    #[inline]
    pub fn len(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }
}

impl<T> DataSize for CountingReceiver<T>
where
    T: DataSize,
{
    const IS_DYNAMIC: bool = T::IS_DYNAMIC;
    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        self.memory_used.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use datasize::DataSize;

    use super::counting_unbounded_channel;

    #[tokio::test]
    async fn test_counting_channel() {
        let item_in_queue_size = size_of::<(usize, u32)>();
        let (tx, mut rc) = counting_unbounded_channel::<u32>();

        assert_eq!(tx.len(), 0);
        assert_eq!(rc.len(), 0);
        assert_eq!(tx.estimate_heap_size(), 0);
        assert_eq!(rc.estimate_heap_size(), 0);

        tx.send_datasized(99).unwrap();
        tx.send_datasized(100).unwrap();
        tx.send_datasized(101).unwrap();
        tx.send_datasized(102).unwrap();
        tx.send_datasized(103).unwrap();

        assert_eq!(tx.len(), 5);
        assert_eq!(rc.len(), 5);

        assert_eq!(tx.estimate_heap_size(), 5 * item_in_queue_size);
        assert_eq!(rc.estimate_heap_size(), 5 * item_in_queue_size);

        rc.recv().await.unwrap();
        rc.recv().await.unwrap();

        assert_eq!(tx.len(), 3);
        assert_eq!(rc.len(), 3);

        assert_eq!(tx.estimate_heap_size(), 3 * item_in_queue_size);
        assert_eq!(rc.estimate_heap_size(), 3 * item_in_queue_size);

        tx.send_datasized(104).unwrap();

        assert_eq!(tx.len(), 4);
        assert_eq!(rc.len(), 4);

        assert_eq!(tx.estimate_heap_size(), 4 * item_in_queue_size);
        assert_eq!(rc.estimate_heap_size(), 4 * item_in_queue_size);
    }
}
