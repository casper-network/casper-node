//! Support for counting channels.
//!
//! Regular tokio channels do not make the number of pending items available. Counting channels wrap
//! regular tokio channels but keep a counter of the number of items up-to-date with every `send`
//! and `recv`. The `len` method can be used to retrieve the number of items.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
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
    sender: UnboundedSender<T>,
    counter: Arc<AtomicUsize>,
    memory_used: Arc<AtomicUsize>,
}

impl<T> CountingSender<T> {
    /// Sends a message down the channel, increasing the count on success.
    #[inline]
    pub fn send(&self, message: T) -> Result<usize, SendError<T>> {
        self.sender
            .send(message)
            .map(|_| self.counter.fetch_add(1, Ordering::SeqCst))
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
        let message_size = message.estimate_heap_size();
        self.send(message).map(|sz| {
            self.memory_used.fetch_add(message_size, Ordering::SeqCst);
            sz
        })
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
    receiver: UnboundedReceiver<T>,
    counter: Arc<AtomicUsize>,
    memory_used: Arc<AtomicUsize>,
}

impl<T> CountingReceiver<T> {
    /// Receives a message from the channel, decreasing the count on success.
    #[inline]
    pub async fn recv(&mut self) -> Option<T> {
        self.receiver.recv().await.map(|value| {
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

impl<T> CountingReceiver<T>
where
    T: DataSize,
{
    /// Receives a message from the channel, decreasing heap memory usage and count.
    #[inline]
    pub async fn recv_datasized(&mut self) -> Option<T> {
        self.recv().await.map(|item| {
            self.memory_used
                .fetch_sub(item.estimate_heap_size(), Ordering::SeqCst);
            item
        })
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

mod tests {
    use datasize::DataSize;

    use super::counting_unbounded_channel;

    #[tokio::test]
    async fn test_counting_channel() {
        let (tx, mut rc) = counting_unbounded_channel::<u32>();

        assert_eq!(tx.len(), 0);
        assert_eq!(rc.len(), 0);
        assert_eq!(tx.estimate_heap_size(), 0);
        assert_eq!(rc.estimate_heap_size(), 0);

        tx.send(99).unwrap();
        tx.send(100).unwrap();
        tx.send(101).unwrap();
        tx.send(102).unwrap();
        tx.send(103).unwrap();

        assert_eq!(tx.len(), 5);
        assert_eq!(rc.len(), 5);

        assert_eq!(tx.estimate_heap_size(), 20);
        assert_eq!(rc.estimate_heap_size(), 20);

        rc.recv().await.unwrap();
        rc.recv().await.unwrap();

        assert_eq!(tx.len(), 3);
        assert_eq!(rc.len(), 3);

        assert_eq!(tx.estimate_heap_size(), 12);
        assert_eq!(rc.estimate_heap_size(), 12);

        tx.send(104).unwrap();

        assert_eq!(tx.len(), 4);
        assert_eq!(rc.len(), 4);

        assert_eq!(tx.estimate_heap_size(), 16);
        assert_eq!(rc.estimate_heap_size(), 16);
    }
}
