//! Backpressured sink and stream.
//!
//! Backpressure is notifying the sender of data that no more data can be sent without the receiver
//! running out of resources to process it.
//!
//! "Natural" backpressure is already built into TCP itself, which has limited send and receive
//! buffers: If a receiver is not reading fast enough, the sender is ultimately forced to buffer
//! more data locally or pause sending.
//!
//! The issue with this type of implementation is that if multiple channels (see [`crate::mux`]) are
//! used across a shared TCP connection, a single blocking channel will block all the other channel
//! (see [Head-of-line blocking](https://en.wikipedia.org/wiki/Head-of-line_blocking)). Furthermore,
//! deadlocks can occur if the data sent is a request which requires a response - should two peers
//! make requests of each other at the same and end up backpressured, they may end up simultaneously
//! waiting for the other peer to make progress.
//!
//! This module allows implementing backpressure over sinks and streams, which can be organized in a
//! multiplexed setup, guaranteed to not be impeding the flow of other channels.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, SinkExt, Stream, StreamExt};

/// A back-pressuring sink.
///
/// Combines a stream of ACKs with a sink that will count requests and expect an appropriate amount
/// of ACKs to flow back through it.
pub struct BackpressuredSink<S, A, Item> {
    inner: S,
    ack_stream: A,
    _phantom: PhantomData<Item>,
    highest_ack: u64,
    last_request: u64, // start at 1
    window_size: u64,
}

impl<S, A, Item> BackpressuredSink<S, A, Item> {
    /// Constructs a new backpressured sink.
    pub fn new(inner: S, ack_stream: A, window_size: u64) -> Self {
        Self {
            inner,
            ack_stream,
            _phantom: PhantomData,
            highest_ack: 0,
            last_request: 1,
            window_size,
        }
    }
}

impl<Item, A, S> Sink<Item> for BackpressuredSink<S, A, Item>
where
    // TODO: `Unpin` trait bounds can be removed by using `map_unchecked` if necessary.
    S: Sink<Item> + Unpin,
    Self: Unpin,
    A: Stream<Item = u64> + Unpin, // TODO: Weave in error from stream.
{
    type Error = <S as Sink<Item>>::Error;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = Pin::into_inner(self);

        // TODO: Describe deadlock-freeness.

        // Attempt to read as many ACKs as possible.
        loop {
            match self_mut.ack_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(new_highest_ack)) => {
                    if new_highest_ack > self_mut.last_request {
                        todo!("got an ACK for a request we did not send");
                    }

                    if new_highest_ack <= self_mut.highest_ack {
                        todo!("got an ACK that is equal or less than a previously received one")
                    }

                    self_mut.highest_ack = new_highest_ack;
                }
                Poll::Ready(None) => {
                    todo!("ACK stream has been closed, exit");
                }
                Poll::Pending => {
                    // We have no more ACKs to read. If we have capacity, we can continue, otherwise
                    // return pending.
                    if self_mut.highest_ack + self_mut.window_size >= self_mut.last_request {
                        break;
                    }

                    return Poll::Pending;
                }
            }
        }

        // We have slots available, it is up to the wrapped sink to accept them.
        self_mut.inner.poll_ready_unpin(cx)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        // We already know there are slots available, increase request count, then forward to sink.
        let self_mut = Pin::into_inner(self);

        self_mut.last_request += 1;

        self_mut.inner.start_send_unpin(item)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.poll_flush_unpin(cx)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().inner.poll_close_unpin(cx)
    }
}
