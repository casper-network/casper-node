//! Support for counting channels.
//!
//! Regular tokio channels do not make the number of pending items available. Counting channels wrap
//! regular tokio channels but keep a counter of the number of items up-to-date with every `send`
//! and `recv`. The `len` method can be used to retrieve the number of items.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use tokio::sync::mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Create an unbounded tokio channel, wrapped in counting sender/receiver.
pub fn counting_unbounded_channel<T>() -> (CountingSender<T>, CountingReceiver<T>) {
    let (tx, rx) = unbounded_channel();
    let counter = Arc::new(AtomicUsize::new(0));

    (
        CountingSender {
            sender: tx,
            counter: counter.clone(),
        },
        CountingReceiver {
            receiver: rx,
            counter,
        },
    )
}

/// Counting sender.
#[derive(Debug)]
pub struct CountingSender<T> {
    sender: UnboundedSender<T>,
    counter: Arc<AtomicUsize>,
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

pub struct CountingReceiver<T> {
    receiver: UnboundedReceiver<T>,
    counter: Arc<AtomicUsize>,
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

#[tokio::test]
async fn test_counting_channel() {
    let (tx, mut rc) = counting_unbounded_channel::<u32>();

    assert_eq!(tx.len(), 0);
    assert_eq!(rc.len(), 0);

    tx.send(99).unwrap();
    tx.send(100).unwrap();
    tx.send(101).unwrap();
    tx.send(102).unwrap();
    tx.send(103).unwrap();

    assert_eq!(tx.len(), 5);
    assert_eq!(rc.len(), 5);

    rc.recv().await.unwrap();
    rc.recv().await.unwrap();

    assert_eq!(tx.len(), 3);
    assert_eq!(rc.len(), 3);

    tx.send(104).unwrap();

    assert_eq!(tx.len(), 4);
    assert_eq!(rc.len(), 4);
}
