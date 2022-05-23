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

use crate::error::Error;

/// A back-pressuring sink.
///
/// Combines a stream `A` of acknoledgements (ACKs) with a sink `S` that will count items in flight
/// and expect an appropriate amount of ACKs to flow back through it.
///
/// In other words, the `BackpressuredSink` will send `window_size` items at most to the sink
/// without expecting to have received one or more ACK through the `ack_stream`.
///
/// The ACKs sent back must be `u64`s, the sink will expect to receive at most one ACK per item
/// sent. The first sent item is expected to receive an ACK of `1u64`, the second `2u64` and so on.
///
/// ACKs may not be sent out of order, but may be combined - an ACK of `n` implicitly indicates ACKs
/// for all previously unsent ACKs less than `n`.
pub struct BackpressuredSink<S, A, Item> {
    /// The inner sink that items will be forwarded to.
    inner: S,
    /// A stream of integers representing ACKs, see struct documentation for details.
    ack_stream: A,
    /// The highest ACK received so far.
    next_expected_ack: u64,
    /// The number of the next request to be sent.
    next_request: u64,
    /// Additional number of items to buffer on inner sink before awaiting ACKs (can be 0, which
    /// still allows for one item).
    window_size: u64,
    /// Phantom data required to include `Item` in the type.
    _phantom: PhantomData<Item>,
}

impl<S, A, Item> BackpressuredSink<S, A, Item> {
    /// Constructs a new backpressured sink.
    ///
    /// `window_size` is the maximum number of additional items to send after the first one without
    /// awaiting ACKs for already sent ones (a size of `0` still allows for one item to be sent).
    pub fn new(inner: S, ack_stream: A, window_size: u64) -> Self {
        Self {
            inner,
            ack_stream,
            next_expected_ack: 1,
            next_request: 0,
            window_size,
            _phantom: PhantomData,
        }
    }
}

impl<Item, A, S> Sink<Item> for BackpressuredSink<S, A, Item>
where
    // TODO: `Unpin` trait bounds can be removed by using `map_unchecked` if necessary.
    S: Sink<Item> + Unpin,
    Self: Unpin,
    A: Stream<Item = u64> + Unpin,
    <S as Sink<Item>>::Error: std::error::Error,
{
    type Error = Error<<S as Sink<Item>>::Error>;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = Pin::into_inner(self);

        // TODO: Describe deadlock-freeness.

        // Attempt to read as many ACKs as possible.
        loop {
            match self_mut.ack_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(highest_ack)) => {
                    if highest_ack >= self_mut.next_request {
                        return Poll::Ready(Err(Error::UnexpectedAck {
                            actual: highest_ack,
                            expected: self_mut.next_expected_ack,
                        }));
                    }

                    if highest_ack < self_mut.next_expected_ack {
                        return Poll::Ready(Err(Error::DuplicateAck {
                            actual: highest_ack,
                            expected: self_mut.next_expected_ack,
                        }));
                    }

                    self_mut.next_expected_ack = highest_ack + 1;
                }
                Poll::Ready(None) => {
                    // The ACK stream has been closed. Close our sink, now that we know, but try to
                    // flush as much as possible.
                    match self_mut.inner.poll_close_unpin(cx).map_err(Error::Sink) {
                        Poll::Ready(Ok(())) => {
                            // All data has been flushed, we can now safely return an error.
                            return Poll::Ready(Err(Error::AckStreamClosed));
                        }
                        Poll::Ready(Err(_)) => {
                            // The was an error polling the ACK stream.
                            return Poll::Ready(Err(Error::AckStreamError));
                        }
                        Poll::Pending => {
                            // Data was flushed, but not done yet, keep polling.
                            return Poll::Pending;
                        }
                    }
                }
                Poll::Pending => {
                    let in_flight = self_mut.next_expected_ack + 1 - self_mut.next_request;

                    // We have no more ACKs to read. If we have capacity, we can continue, otherwise
                    // return pending.
                    if in_flight <= self_mut.window_size {
                        break;
                    }

                    return Poll::Pending;
                }
            }
        }

        // We have slots available, it is up to the wrapped sink to accept them.
        self_mut.inner.poll_ready_unpin(cx).map_err(Error::Sink)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        // We already know there are slots available, increase request count, then forward to sink.
        let self_mut = Pin::into_inner(self);

        self_mut.next_request += 1;

        self_mut.inner.start_send_unpin(item).map_err(Error::Sink)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut()
            .inner
            .poll_flush_unpin(cx)
            .map_err(Error::Sink)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut()
            .inner
            .poll_close_unpin(cx)
            .map_err(Error::Sink)
    }
}
