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
//! used across a shared TCP connection, a single blocking channel will block all the other channels
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
/// without having received one or more ACKs through the `ack_stream`.
///
/// The ACKs sent back must be `u64`s, the sink will expect to receive at most one ACK per item
/// sent. The first sent item is expected to receive an ACK of `1u64`, the second `2u64` and so on.
///
/// ACKs are not acknowledgments for a specific item being processed but indicate the total number
/// of processed items instead, thus they are unordered. They may be combined, an ACK of `n` implies
/// all missing ACKs `< n`.
pub struct BackpressuredSink<S, A, Item> {
    /// The inner sink that items will be forwarded to.
    inner: S,
    /// A stream of integers representing ACKs, see struct documentation for details.
    ack_stream: A,
    /// The highest ACK received so far.
    received_ack: u64,
    /// The number of the next request to be sent.
    last_request: u64,
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
            received_ack: 0,
            last_request: 0,
            window_size,
            _phantom: PhantomData,
        }
    }

    /// Deconstructs a backpressured sink into its components.
    pub fn into_inner(self) -> (S, A) {
        (self.inner, self.ack_stream)
    }
}

impl<Item, A, S> Sink<Item> for BackpressuredSink<S, A, Item>
where
    // TODO: `Unpin` trait bounds can be
    // removed by using `map_unchecked` if
    // necessary.
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
                Poll::Ready(Some(ack_received)) => {
                    if ack_received > self_mut.last_request {
                        return Poll::Ready(Err(Error::UnexpectedAck {
                            actual: ack_received,
                            items_sent: self_mut.last_request,
                        }));
                    }

                    if ack_received <= self_mut.received_ack {
                        return Poll::Ready(Err(Error::DuplicateAck {
                            ack_received,
                            highest: self_mut.received_ack,
                        }));
                    }

                    self_mut.received_ack = ack_received;
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
                    // Invariant: `received_ack` is always <= `last_request`.
                    let in_flight = self_mut.last_request - self_mut.received_ack;

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

        self_mut.last_request += 1;

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

#[cfg(test)]
mod tests {
    use futures::{FutureExt, SinkExt};
    use tokio::sync::mpsc::UnboundedSender;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    use crate::error::Error;

    use super::BackpressuredSink;

    /// Window size used in tests.
    const WINDOW_SIZE: u64 = 3;

    /// A set of fixtures commonly used in the backpressure tests below.
    struct Fixtures {
        /// The stream ACKs are sent into.
        ack_sender: UnboundedSender<u64>,
        /// The backpressured sink.
        bp: BackpressuredSink<Vec<char>, UnboundedReceiverStream<u64>, char>,
    }

    impl Fixtures {
        /// Creates a new set of fixtures.
        fn new() -> Self {
            let sink = Vec::new();
            let (ack_sender, ack_receiver) = tokio::sync::mpsc::unbounded_channel::<u64>();
            let ack_stream = UnboundedReceiverStream::new(ack_receiver);
            let bp = BackpressuredSink::new(sink, ack_stream, WINDOW_SIZE);

            Fixtures { ack_sender, bp }
        }
    }

    #[test]
    fn backpressure_lifecycle() {
        let Fixtures { ack_sender, mut bp } = Fixtures::new();

        // The first four attempts at `window_size = 3` should succeed.
        bp.send('A').now_or_never().unwrap().unwrap();
        bp.send('B').now_or_never().unwrap().unwrap();
        bp.send('C').now_or_never().unwrap().unwrap();
        bp.send('D').now_or_never().unwrap().unwrap();

        // The fifth attempt will fail, due to no ACKs having been received.
        assert!(bp.send('E').now_or_never().is_none());

        // We can now send some ACKs.
        ack_sender.send(1).unwrap();

        // Retry sending the fifth message, sixth should still block.
        bp.send('E').now_or_never().unwrap().unwrap();
        assert!(bp.send('F').now_or_never().is_none());

        // Send a combined ack for three messages.
        ack_sender.send(4).unwrap();

        // This allows 3 more messages to go in.
        bp.send('F').now_or_never().unwrap().unwrap();
        bp.send('G').now_or_never().unwrap().unwrap();
        bp.send('H').now_or_never().unwrap().unwrap();
        assert!(bp.send('I').now_or_never().is_none());

        // Send more ACKs to ensure we also get errors if there is capacity.
        ack_sender.send(6).unwrap();

        // We can now close the ACK stream to check if the sink errors after that.
        drop(ack_sender);

        assert!(matches!(
            bp.send('I').now_or_never(),
            Some(Err(Error::AckStreamClosed))
        ));

        // Check all data was received correctly.
        let output: String = bp.into_inner().0.into_iter().collect();

        assert_eq!(output, "ABCDEFGH");
    }

    #[test]
    fn ensure_premature_ack_kills_stream() {
        let Fixtures { ack_sender, mut bp } = Fixtures::new();

        bp.send('A').now_or_never().unwrap().unwrap();
        bp.send('B').now_or_never().unwrap().unwrap();
        ack_sender.send(3).unwrap();

        assert!(matches!(
            bp.send('C').now_or_never(),
            Some(Err(Error::UnexpectedAck {
                items_sent: 2,
                actual: 3
            }))
        ));
    }

    #[test]
    fn ensure_redundant_ack_kills_stream() {
        let Fixtures { ack_sender, mut bp } = Fixtures::new();

        bp.send('A').now_or_never().unwrap().unwrap();
        bp.send('B').now_or_never().unwrap().unwrap();
        ack_sender.send(2).unwrap();
        ack_sender.send(1).unwrap();

        assert!(matches!(
            bp.send('C').now_or_never(),
            Some(Err(Error::DuplicateAck {
                ack_received: 1,
                highest: 2
            }))
        ));
    }
}
