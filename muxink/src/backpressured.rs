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
//! ([Head-of-line blocking](https://en.wikipedia.org/wiki/Head-of-line_blocking)). Furthermore,
//! deadlocks can occur if the data sent is a request which requires a response - should two peers
//! make requests of each other at the same and end up backpressured, they may end up simultaneously
//! waiting for the other peer to make progress.
//!
//! This module allows implementing backpressure over sinks and streams, which can be organized in a
//! multiplexed setup, guaranteed to not be impeding the flow of other channels.

use std::{
    cmp::max,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    channel::mpsc::{Receiver, Sender},
    ready, Sink, SinkExt, Stream, StreamExt,
};
use thiserror::Error;
use tracing::error;

use crate::try_ready;

/// A backpressuring sink.
///
/// Combines a stream `A` of acknoledgements (ACKs) with a sink `S` that will count items in flight
/// and expect an appropriate amount of ACKs to flow back through it.
///
/// The `BackpressuredSink` will pass `window_size` items at most to the wrapped sink without having
/// received one or more ACKs through the `ack_stream`. If this limit is exceeded, the sink polls as
/// pending.
///
/// The ACKs sent back must be `u64`s, the sink will expect to receive at most one ACK per item
/// sent. The first sent item is expected to receive an ACK of `1u64`, the second `2u64` and so on.
///
/// ACKs are not acknowledgments for a specific item being processed but indicate the total number
/// of processed items instead, thus they are unordered. They may be combined, an ACK of `n` implies
/// all missing ACKs `< n`.
///
/// Duplicate ACKs will cause an error, thus sending ACKs in the wrong order will cause an error in
/// the sink, as the higher ACK will implicitly have contained the lower one.
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

/// A backpressure error.
#[derive(Debug, Error)]
pub enum BackpressureError<SinkErr, AckErr>
where
    SinkErr: std::error::Error,
    AckErr: std::error::Error,
{
    /// An ACK was received for an item that had not been sent yet.
    #[error("received ACK {actual}, but only sent {items_sent} items")]
    UnexpectedAck { actual: u64, items_sent: u64 },
    /// Received an ACK for an item that an ACK must have already been received
    /// as it is outside the window.
    #[error("duplicate ACK {ack_received} received, already received {highest}")]
    DuplicateAck { ack_received: u64, highest: u64 },
    /// The ACK stream associated with a backpressured channel was closed.
    #[error("ACK stream closed")]
    AckStreamClosed,
    /// There was an error retrieving ACKs from the ACK stream.
    #[error("ACK stream error")]
    AckStreamError(#[source] AckErr),
    /// The underlying sink had an error.
    #[error(transparent)]
    Sink(#[from] SinkErr),
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

    /// Validates a received ack.
    ///
    /// Returns an error if the `ACK` was a duplicate or from the future.
    fn validate_ack<SinkErr, AckErr>(
        &mut self,
        ack_received: u64,
    ) -> Result<(), BackpressureError<SinkErr, AckErr>>
    where
        SinkErr: std::error::Error,
        AckErr: std::error::Error,
    {
        if ack_received > self.last_request {
            return Err(BackpressureError::UnexpectedAck {
                actual: ack_received,
                items_sent: self.last_request,
            });
        }

        if ack_received + self.window_size < self.last_request {
            return Err(BackpressureError::DuplicateAck {
                ack_received,
                highest: self.received_ack,
            });
        }

        Ok(())
    }
}

impl<Item, A, S, AckErr> Sink<Item> for BackpressuredSink<S, A, Item>
where
    // TODO: `Unpin` trait bounds
    // can be removed by using
    // `map_unchecked` if
    // necessary.
    S: Sink<Item> + Unpin,
    Self: Unpin,
    A: Stream<Item = Result<u64, AckErr>> + Unpin,
    AckErr: std::error::Error,
    <S as Sink<Item>>::Error: std::error::Error,
{
    type Error = BackpressureError<<S as Sink<Item>>::Error, AckErr>;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = Pin::into_inner(self);

        // Attempt to read as many ACKs as possible.
        loop {
            match self_mut.ack_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Err(ack_err))) => {
                    return Poll::Ready(Err(BackpressureError::AckStreamError(ack_err)))
                }
                Poll::Ready(Some(Ok(ack_received))) => {
                    try_ready!(self_mut.validate_ack(ack_received));
                    self_mut.received_ack = max(self_mut.received_ack, ack_received);
                }
                Poll::Ready(None) => {
                    return Poll::Ready(Err(BackpressureError::AckStreamClosed));
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
        self_mut
            .inner
            .poll_ready_unpin(cx)
            .map_err(BackpressureError::Sink)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        // We already know there are slots available, increase request count, then forward to sink.
        let self_mut = Pin::into_inner(self);

        self_mut.last_request += 1;

        self_mut
            .inner
            .start_send_unpin(item)
            .map_err(BackpressureError::Sink)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut()
            .inner
            .poll_flush_unpin(cx)
            .map_err(BackpressureError::Sink)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut()
            .inner
            .poll_close_unpin(cx)
            .map_err(BackpressureError::Sink)
    }
}

/// A ticket from a [`BackpressuredStream`].
///
/// Each ticket, when dropped, will queue an ACK to be sent the next time the stream is polled.
///
/// When the stream that created the ticket is dropped before the ticket, the ACK associated with
/// the ticket is silently ignored.
#[derive(Debug)]
pub struct Ticket {
    sender: Sender<()>,
}

impl Ticket {
    /// Creates a new ticket with the cloned `Sender` from the original
    /// [`BackpressuredStream`].
    pub fn new(sender: Sender<()>) -> Self {
        Self { sender }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        // Signal to the stream that the associated item has been processed
        // and capacity should increase.
        if let Err(e) = self.sender.try_send(()) {
            // `try_send` can fail if either the buffer is full or the receiver
            // was dropped. In the case of a receiver drop, we silently ignore
            // the error as there is nothing to notify anymore.
            if e.is_full() {
                error!("Backpressured stream exceeded window size, ACK channel is full.");
            }
        }
    }
}

/// Error type for a [`BackpressuredStream`].
#[derive(Debug, Error)]
pub enum BackpressuredStreamError<ErrRecv: std::error::Error, ErrSendAck: std::error::Error> {
    /// Couldn't enqueue an ACK for sending on the ACK sink after it polled ready.
    #[error("error sending ACK")]
    AckSend(#[source] ErrSendAck),
    /// Error on polling the ACK sink.
    #[error("error polling the ACK stream")]
    AckSinkPoll,
    /// Error flushing the ACK sink.
    #[error("error flushing the ACK stream")]
    Flush,
    /// The peer exceeded the configure window size.
    #[error("peer exceeded window size")]
    ItemOverflow,
    /// Error encountered by the underlying stream.
    #[error("stream receive failure")]
    Stream(#[source] ErrRecv),
}

/// A backpressuring stream.
///
/// Combines a sink `A` of acknowledgements (ACKs) with a stream `S` that will allow a maximum
/// number of items in flight and send ACKs back to signal availability. Sending of ACKs is managed
/// through [`Ticket`]s, which will automatically trigger an ACK being sent when dropped.
///
/// If more than `window_size` items are received on the stream before ACKs have been sent back, the
/// stream will return an error indicating the peer's capacity violation.
///
/// If a stream is dropped, any outstanding ACKs will be lost. No ACKs will be sent unless this
/// stream is actively polled (e.g. via [`StreamExt::next`](futures::stream::StreamExt::next)).
pub struct BackpressuredStream<S, A, Item> {
    /// Inner stream to which backpressure is added.
    inner: S,
    /// Sink where the stream sends the ACKs to the sender. Users should ensure
    /// this sink is able to buffer `window_size` + 1 ACKs in order to avoid
    /// unnecessary latency related to flushing when sending ACKs back to the
    /// sender.
    ack_sink: A,
    /// Receiving end of ACK channel between the yielded tickets and the
    /// [`BackpressuredStream`]. ACKs received here will then be forwarded to
    /// the sender through `ack_stream`.
    ack_receiver: Receiver<()>,
    /// Sending end of ACK channel between the yielded tickets and the
    /// [`BackpressuredStream`]. This sender will be cloned and yielded in the
    /// form of a ticket along with items from the inner stream.
    ack_sender: Sender<()>,
    /// Counter of items processed.
    items_processed: u64,
    /// Counter of items received from the underlying stream.
    last_received: u64,
    /// Counter of ACKs received from yielded tickets.
    acks_received: u64,
    /// The maximum number of items the stream can process at a single point
    /// in time.
    window_size: u64,
    /// Phantom data required to include `Item` in the type.
    _phantom: PhantomData<Item>,
}

impl<S, A, Item> BackpressuredStream<S, A, Item> {
    /// Creates a new [`BackpressuredStream`] with a window size from a given
    /// stream and ACK sink.
    pub fn new(inner: S, ack_sink: A, window_size: u64) -> Self {
        // Create the channel used by tickets to signal that items are done
        // processing. The channel will have a buffer of size `window_size + 1`
        // as a `BackpressuredStream` with a window size of 0 should still be
        // able to yield one item at a time.
        let (ack_sender, ack_receiver) = futures::channel::mpsc::channel(window_size as usize + 1);
        Self {
            inner,
            ack_sink,
            ack_receiver,
            ack_sender,
            items_processed: 0,
            last_received: 0,
            acks_received: 0,
            window_size,
            _phantom: PhantomData,
        }
    }
}

impl<S, A, StreamItem, E> Stream for BackpressuredStream<S, A, StreamItem>
where
    S: Stream<Item = Result<StreamItem, E>> + Unpin,
    E: std::error::Error,
    Self: Unpin,
    A: Sink<u64> + Unpin,
    <A as Sink<u64>>::Error: std::error::Error,
{
    type Item = Result<(StreamItem, Ticket), BackpressuredStreamError<E, <A as Sink<u64>>::Error>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();
        // Retrieve every ACK from `ack_receiver`.
        loop {
            match self_mut.ack_receiver.poll_next_unpin(cx) {
                Poll::Ready(Some(_)) => {
                    // Add to the received ACK counter.
                    self_mut.acks_received += 1;
                }
                // If there are no more ACKs waiting in the receiver,
                // move on to sending anything received so far.
                Poll::Pending => break,
                // This is actually unreachable since the ACK stream
                // will return `Poll::Ready(None)` only when all the
                // senders are dropped, but one sender is always held
                // within this struct.
                Poll::Ready(None) => return Poll::Ready(None),
            }
        }

        // If there are received ACKs, proceed to enqueue them for sending.
        if self_mut.acks_received > 0 {
            // Ensure the ACK sink is ready to accept new ACKs.
            match self_mut.ack_sink.poll_ready_unpin(cx) {
                Poll::Ready(Ok(_)) => {
                    // Update the number of processed items. Items are considered
                    // processed at this point even though they haven't been
                    // flushed yet. From the point of view of a
                    // `BackpressuredStream`, the resources of the associated
                    // messages have been freed, so there is available capacity
                    // for more messages.
                    self_mut.items_processed += self_mut.acks_received;
                    // Enqueue one item representing the number of items processed
                    // so far. This should never be an error as the sink must be
                    // ready to accept new items at this point.
                    if let Err(err) = self_mut.ack_sink.start_send_unpin(self_mut.items_processed) {
                        return Poll::Ready(Some(Err(BackpressuredStreamError::AckSend(err))));
                    }
                    // Now that the ACKs have been handed to the ACK sink,
                    // reset the received ACK counter.
                    self_mut.acks_received = 0;
                }
                Poll::Ready(Err(_)) => {
                    // Return the error on the ACK sink.
                    return Poll::Ready(Some(Err(BackpressuredStreamError::AckSinkPoll)));
                }
                Poll::Pending => {
                    // Even though the sink is not ready to accept new items,
                    // the ACKs received from dropped tickets mean the stream
                    // has available capacity to accept new items. Any ACKs
                    // received from tickets are buffered in `acks_received`
                    // and will eventually be sent.
                }
            }
        }

        // After ensuring all possible ACKs have been received and handed to
        // the ACK sink, look to accept new items from the underlying stream.
        // If the stream is pending, then this backpressured stream is also
        // pending.
        match ready!(self_mut.inner.poll_next_unpin(cx)) {
            Some(Ok(next_item)) => {
                // After receiving an item, ensure the maximum number of
                // in-flight items does not exceed the window size.
                if self_mut.last_received > self_mut.items_processed + self_mut.window_size {
                    return Poll::Ready(Some(Err(BackpressuredStreamError::ItemOverflow)));
                }
                // Update the counter of received items.
                self_mut.last_received += 1;
                // Yield the item along with a ticket to be released when
                // the processing of said item is done.
                Poll::Ready(Some(Ok((
                    next_item,
                    Ticket::new(self_mut.ack_sender.clone()),
                ))))
            }
            Some(Err(err)) => {
                // Return the error on the underlying stream.
                Poll::Ready(Some(Err(BackpressuredStreamError::Stream(err))))
            }
            None => {
                // If the underlying stream is closed, the `BackpressuredStream`
                // is also considered closed. Polling the stream after this point
                // is undefined behavior.
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, convert::Infallible, sync::Arc};

    use bytes::Bytes;
    use futures::{FutureExt, Sink, SinkExt, Stream, StreamExt};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_util::sync::PollSender;

    use crate::testing::{
        collect_bufs,
        encoding::{EncodeAndSend, TestEncodeable},
        testing_sink::{TestingSink, TestingSinkRef},
    };

    use super::{
        BackpressureError, BackpressuredSink, BackpressuredStream, BackpressuredStreamError,
    };

    /// Window size used in tests.
    const WINDOW_SIZE: u64 = 3;

    /// Sets up a `Sink`/`Stream` pair that outputs infallible results.
    fn setup_io_pipe<T: Send + Sync + 'static>(
        size: usize,
    ) -> (
        impl Sink<T, Error = Infallible> + Unpin + 'static,
        impl Stream<Item = Result<T, Infallible>> + Unpin + 'static,
    ) {
        let (send, recv) = tokio::sync::mpsc::channel::<T>(size);

        let stream = ReceiverStream::new(recv).map(Ok);

        let sink =
            PollSender::new(send).sink_map_err(|_err| panic!("did not expect a `PollSendError`"));

        (sink, stream)
    }

    /// A common set of fixtures used in the backpressure tests.
    ///
    /// The fixtures represent what a server holds when dealing with a backpressured client.
    struct OneWayFixtures {
        /// A sender for ACKs back to the client.
        ack_sink: Box<dyn Sink<u64, Error = Infallible> + Unpin>,
        /// The clients sink for requests, with no backpressure wrapper. Used for retrieving the
        /// test data in the end or setting plugged/clogged status.
        sink: Arc<TestingSink>,
        /// The properly set up backpressured sink.
        bp: BackpressuredSink<
            TestingSinkRef,
            Box<dyn Stream<Item = Result<u64, Infallible>> + Unpin>,
            Bytes,
        >,
    }

    impl OneWayFixtures {
        /// Creates a new set of fixtures.
        fn new() -> Self {
            let sink = Arc::new(TestingSink::new());

            let (raw_ack_sink, raw_ack_stream) = setup_io_pipe::<u64>(1024);

            // The ACK stream and sink need to be boxed to make their types named.
            let ack_sink: Box<dyn Sink<u64, Error = Infallible> + Unpin> = Box::new(raw_ack_sink);
            let ack_stream: Box<dyn Stream<Item = Result<u64, Infallible>> + Unpin> =
                Box::new(raw_ack_stream);

            let bp = BackpressuredSink::new(sink.clone().into_ref(), ack_stream, WINDOW_SIZE);

            Self { ack_sink, sink, bp }
        }
    }

    /// A more complicated setup for testing backpressure that allows accessing both sides of the
    /// connection.
    ///
    /// The resulting `client` sends byte frames across to the `server`, with ACKs flowing through
    /// the associated ACK pipe.
    #[allow(clippy::type_complexity)]
    struct TwoWayFixtures {
        client: BackpressuredSink<
            Box<dyn Sink<Bytes, Error = Infallible> + Send + Unpin>,
            Box<dyn Stream<Item = Result<u64, Infallible>> + Send + Unpin>,
            Bytes,
        >,
        server: BackpressuredStream<
            Box<dyn Stream<Item = Result<Bytes, Infallible>> + Send + Unpin>,
            Box<dyn Sink<u64, Error = Infallible> + Send + Unpin>,
            Bytes,
        >,
    }

    impl TwoWayFixtures {
        /// Creates a new set of two-way fixtures.
        fn new(size: usize) -> Self {
            let (sink, stream) = setup_io_pipe::<Bytes>(size);

            let (ack_sink, ack_stream) = setup_io_pipe::<u64>(size);

            let boxed_sink: Box<dyn Sink<Bytes, Error = Infallible> + Send + Unpin + 'static> =
                Box::new(sink);
            let boxed_ack_stream: Box<dyn Stream<Item = Result<u64, Infallible>> + Send + Unpin> =
                Box::new(ack_stream);

            let client = BackpressuredSink::new(boxed_sink, boxed_ack_stream, WINDOW_SIZE);

            let boxed_stream: Box<dyn Stream<Item = Result<Bytes, Infallible>> + Send + Unpin> =
                Box::new(stream);
            let boxed_ack_sink: Box<dyn Sink<u64, Error = Infallible> + Send + Unpin> =
                Box::new(ack_sink);
            let server = BackpressuredStream::new(boxed_stream, boxed_ack_sink, WINDOW_SIZE);

            TwoWayFixtures { client, server }
        }
    }

    #[test]
    fn backpressured_sink_lifecycle() {
        let OneWayFixtures {
            mut ack_sink,
            sink,
            mut bp,
        } = OneWayFixtures::new();

        // The first four attempts at `window_size = 3` should succeed.
        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        bp.encode_and_send('C').now_or_never().unwrap().unwrap();
        bp.encode_and_send('D').now_or_never().unwrap().unwrap();

        // The fifth attempt will fail, due to no ACKs having been received.
        assert!(bp.encode_and_send('E').now_or_never().is_none());

        // We can now send some ACKs.
        ack_sink.send(1).now_or_never().unwrap().unwrap();

        // Retry sending the fifth message, sixth should still block.
        bp.encode_and_send('E').now_or_never().unwrap().unwrap();
        assert!(bp.encode_and_send('F').now_or_never().is_none());

        // Send a combined ack for three messages.
        ack_sink.send(4).now_or_never().unwrap().unwrap();

        // This allows 3 more messages to go in.
        bp.encode_and_send('F').now_or_never().unwrap().unwrap();
        bp.encode_and_send('G').now_or_never().unwrap().unwrap();
        bp.encode_and_send('H').now_or_never().unwrap().unwrap();
        assert!(bp.encode_and_send('I').now_or_never().is_none());

        // Send more ACKs to ensure we also get errors if there is capacity.
        ack_sink.send(6).now_or_never().unwrap().unwrap();

        // We can now close the ACK stream to check if the sink errors after that.
        drop(ack_sink);

        assert!(matches!(
            bp.encode_and_send('I').now_or_never(),
            Some(Err(BackpressureError::AckStreamClosed))
        ));

        // Check all data was received correctly.
        assert_eq!(sink.get_contents_string(), "ABCDEFGH");
    }

    #[test]
    fn backpressured_stream_lifecycle() {
        let (sink, stream) = tokio::sync::mpsc::channel::<u8>(u8::MAX as usize);
        let (ack_sender, mut ack_receiver) = tokio::sync::mpsc::channel::<u64>(u8::MAX as usize);

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u8, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        // The first four attempts at `window_size = 3` should succeed.
        sink.send(0).now_or_never().unwrap().unwrap();
        sink.send(1).now_or_never().unwrap().unwrap();
        sink.send(2).now_or_never().unwrap().unwrap();
        sink.send(3).now_or_never().unwrap().unwrap();

        let mut items = VecDeque::new();
        let mut tickets = VecDeque::new();
        // Receive the 4 items we sent along with their tickets.
        for _ in 0..4 {
            let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
            items.push_back(item);
            tickets.push_back(ticket);
        }
        // Make sure there are no AKCs to receive as the tickets have not been
        // dropped yet.
        assert!(ack_receiver.recv().now_or_never().is_none());

        // Drop the first ticket.
        let _ = tickets.pop_front();
        // Poll the stream to propagate the ticket drop.
        assert!(stream.next().now_or_never().is_none());

        // We should be able to send a new item now that one ticket has been
        // dropped.
        sink.send(4).now_or_never().unwrap().unwrap();
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        items.push_back(item);
        tickets.push_back(ticket);

        // Drop another ticket.
        let _ = tickets.pop_front();

        // Send a new item without propagating the ticket drop through a poll.
        // This should work because the ACKs are handled first in the poll
        // state machine.
        sink.send(5).now_or_never().unwrap().unwrap();
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        items.push_back(item);
        tickets.push_back(ticket);

        // Sending another item when the stream is at full capacity should
        // yield an error from the stream.
        sink.send(6).now_or_never().unwrap().unwrap();
        assert!(stream.next().now_or_never().unwrap().unwrap().is_err());
    }

    #[test]
    fn backpressured_roundtrip() {
        let TwoWayFixtures {
            mut client,
            mut server,
        } = TwoWayFixtures::new(1024);

        // This test assumes a hardcoded window size of 3.
        assert_eq!(WINDOW_SIZE, 3);

        // Send just enough requests to max out the receive window of the backpressured channel.
        for i in 0..=3u8 {
            client.encode_and_send(i).now_or_never().unwrap().unwrap();
        }

        // Sanity check: Attempting to send another item will be refused by the client side's
        // limiter to avoid exceeding the allowed window.
        assert!(client.encode_and_send(99_u8).now_or_never().is_none());

        let mut items = VecDeque::new();
        let mut tickets = VecDeque::new();

        // Receive the items along with their tickets all at once.
        for _ in 0..=WINDOW_SIZE as u8 {
            let (item, ticket) = server.next().now_or_never().unwrap().unwrap().unwrap();
            items.push_back(item);
            tickets.push_back(ticket);
        }

        // We simulate the completion of two items by dropping their tickets.
        let _ = tickets.pop_front();
        let _ = tickets.pop_front();

        // Send the ACKs to the client by polling the server.
        assert_eq!(server.items_processed, 0); // (Before, the internal channel will not have been polled).
        assert_eq!(server.last_received, 4);
        assert!(server.next().now_or_never().is_none());
        assert_eq!(server.last_received, 4);
        assert_eq!(server.items_processed, 2);

        // Send another item. ACKs will be received at the start, so while it looks like as if we
        // cannot send the item initially, the incoming ACK(2) will fix this.
        assert_eq!(client.last_request, 4);
        assert_eq!(client.received_ack, 0);
        client.encode_and_send(4u8).now_or_never().unwrap().unwrap();
        assert_eq!(client.last_request, 5);
        assert_eq!(client.received_ack, 2);
        assert_eq!(server.items_processed, 2);

        // Send another item, filling up the entire window again.
        client.encode_and_send(5u8).now_or_never().unwrap().unwrap();
        assert_eq!(client.last_request, 6);

        // Receive two additional items.
        for _ in 0..2 {
            let (item, ticket) = server.next().now_or_never().unwrap().unwrap().unwrap();
            items.push_back(item);
            tickets.push_back(ticket);
        }

        // At this point client and server should reflect the same state.
        assert_eq!(client.last_request, 6);
        assert_eq!(client.received_ack, 2);
        assert_eq!(server.last_received, 6);
        assert_eq!(server.items_processed, 2);

        // Drop all tickets, marking the work as done.
        tickets.clear();

        // The ACKs have been queued now, send them by polling the server.
        assert!(server.next().now_or_never().is_none());
        // Make sure the server state reflects the sent ACKs.
        assert_eq!(server.items_processed, 6);

        // Send another item.
        client.encode_and_send(6u8).now_or_never().unwrap().unwrap();
        assert_eq!(client.received_ack, 6);
        assert_eq!(client.last_request, 7);

        // Receive the item.
        let (item, ticket) = server.next().now_or_never().unwrap().unwrap().unwrap();
        assert_eq!(server.items_processed, 6);
        assert_eq!(server.last_received, 7);
        items.push_back(item);
        tickets.push_back(ticket);

        // Send two items.
        client.encode_and_send(7u8).now_or_never().unwrap().unwrap();
        client.encode_and_send(8u8).now_or_never().unwrap().unwrap();
        // Receive only one item.
        let (item, ticket) = server.next().now_or_never().unwrap().unwrap().unwrap();
        // The client state should be ahead of the server by one item, which is yet to be yielded in
        // a `poll_next` by the server.
        items.push_back(item);
        tickets.push_back(ticket);

        // Two items are on the server processing, one is in transit:
        assert_eq!(tickets.len(), 2);
        assert_eq!(client.last_request, 9);
        assert_eq!(client.received_ack, 6);
        assert_eq!(server.items_processed, 6);
        assert_eq!(server.last_received, 8);

        // Finish processing another item.
        let _ = tickets.pop_front();
        // Receive the other item. This will implicitly send the ACK from the popped ticket.
        let (item, ticket) = server.next().now_or_never().unwrap().unwrap().unwrap();
        // Ensure the stream state has been updated.
        assert_eq!(server.items_processed, 7);
        assert_eq!(server.last_received, 9);
        items.push_back(item);
        tickets.push_back(ticket);

        // The server should have received all of these items so far.
        assert_eq!(
            collect_bufs(items.clone().into_iter()),
            b"\x00\x01\x02\x03\x04\x05\x06\x07\x08"
        );

        // Now send two more items to occupy the entire window. In between, the client should have
        // received the latest ACK with this poll, so we check it against the stream one to ensure
        // correctness.
        client.encode_and_send(9u8).now_or_never().unwrap().unwrap();
        assert_eq!(client.received_ack, server.items_processed);
        client
            .encode_and_send(10u8)
            .now_or_never()
            .unwrap()
            .unwrap();
        // Make sure we reached full capacity in the sink state.
        assert_eq!(client.last_request, client.received_ack + 3 + 1);
        // Sending a new item should return `Poll::Pending`.
        assert!(client.encode_and_send(9u8).now_or_never().is_none());
    }

    #[test]
    fn backpressured_sink_premature_ack_kills_stream() {
        let OneWayFixtures {
            mut ack_sink,
            mut bp,
            ..
        } = OneWayFixtures::new();

        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        ack_sink.send(3).now_or_never().unwrap().unwrap();

        assert!(matches!(
            bp.encode_and_send('C').now_or_never(),
            Some(Err(BackpressureError::UnexpectedAck {
                items_sent: 2,
                actual: 3
            }))
        ));
    }

    #[test]
    fn backpressured_sink_redundant_ack_kills_stream() {
        // Window size is 3, so if the sink can send at most
        // `window_size + 1` requests, it must also follow that any ACKs fall
        // in the [`last_request` - `window_size` - 1, `last_request`]
        // interval. In other words, if we sent request no. `last_request`,
        // we must have had ACKs up until at least
        // `last_request` - `window_size`, so an ACK out of range is a
        // duplicate.
        let OneWayFixtures {
            mut ack_sink,
            mut bp,
            ..
        } = OneWayFixtures::new();

        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        // Out of order ACKs work.
        ack_sink.send(2).now_or_never().unwrap().unwrap();
        ack_sink.send(1).now_or_never().unwrap().unwrap();
        // Send 3 more items to make it 5 in total.
        bp.encode_and_send('C').now_or_never().unwrap().unwrap();
        bp.encode_and_send('D').now_or_never().unwrap().unwrap();
        bp.encode_and_send('E').now_or_never().unwrap().unwrap();
        // Send a duplicate ACK of 1, which is outside the allowed range.
        ack_sink.send(1).now_or_never().unwrap().unwrap();

        assert!(matches!(
            bp.encode_and_send('F').now_or_never(),
            Some(Err(BackpressureError::DuplicateAck {
                ack_received: 1,
                highest: 2
            }))
        ));
    }

    #[test]
    fn backpressured_sink_exceeding_window_kills_stream() {
        let TwoWayFixtures {
            mut client,
            mut server,
        } = TwoWayFixtures::new(512);

        // Fill up the receive window.
        for _ in 0..=WINDOW_SIZE {
            client.encode_and_send('X').now_or_never().unwrap().unwrap();
        }

        // The "overflow" should be rejected.
        assert!(client.encode_and_send('X').now_or_never().is_none());

        // Deconstruct the client, forcing another packet onto "wire".
        let (mut sink, _ack_stream) = client.into_inner();

        sink.encode_and_send('P').now_or_never().unwrap().unwrap();

        // Now we can look at the server side.
        let mut in_progress = Vec::new();
        for _ in 0..=WINDOW_SIZE {
            let received = server.next().now_or_never().unwrap().unwrap();
            let (_bytes, ticket) = received.unwrap();

            // We need to keep the tickets around to simulate the server being busy.
            in_progress.push(ticket);
        }

        // Now the server should notice that the backpressure limit has been exceeded and return an
        // error.
        let overflow_err = server.next().now_or_never().unwrap().unwrap().unwrap_err();
        assert!(matches!(
            overflow_err,
            BackpressuredStreamError::ItemOverflow
        ));
    }

    #[tokio::test]
    async fn backpressured_sink_concurrent_tasks() {
        let to_send: Vec<u16> = (0..u16::MAX).into_iter().rev().collect();

        let TwoWayFixtures {
            mut client,
            mut server,
        } = TwoWayFixtures::new(512);

        let send_fut = tokio::spawn(async move {
            for item in to_send.iter() {
                // Try to feed each item into the sink.
                if client.feed(item.encode()).await.is_err() {
                    // When `feed` fails, the sink is full, so we flush it.
                    client.flush().await.unwrap();
                    // After flushing, the sink must be able to accept new items.
                    client.feed(item.encode()).await.unwrap();
                }
            }
            // Close the sink here to signal the end of the stream on the other end.
            client.close().await.unwrap();
            // Return the sink so we don't drop the ACK sending end yet.
            client
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u16> = vec![];
            while let Some((item, ticket)) = server.next().await.transpose().unwrap() {
                // Receive each item sent by the sink.
                items.push(u16::decode(&item));
                // Send the ACK for it.
                drop(ticket);
            }
            items
        });

        let (send_result, recv_result) = tokio::join!(send_fut, recv_fut);
        assert!(send_result.is_ok());
        assert_eq!(
            recv_result.unwrap(),
            (0..u16::MAX).into_iter().rev().collect::<Vec<u16>>()
        );
    }

    #[tokio::test]
    async fn backpressured_roundtrip_concurrent_tasks() {
        let to_send: Vec<u16> = (0..u16::MAX).into_iter().rev().collect();
        let TwoWayFixtures {
            mut client,
            mut server,
        } = TwoWayFixtures::new(512);

        let send_fut = tokio::spawn(async move {
            for item in to_send.iter() {
                // Try to feed each item into the sink.
                if client.feed(item.encode()).await.is_err() {
                    // When `feed` fails, the sink is full, so we flush it.
                    client.flush().await.unwrap();
                    // After flushing, the sink must be able to accept new items.
                    match client.feed(item.encode()).await {
                        Err(BackpressureError::AckStreamClosed) => {
                            return client;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            panic!("Error on sink send: {}", e);
                        }
                    }
                }
            }
            // Close the sink here to signal the end of the stream on the other end.
            client.close().await.unwrap();
            // Return the sink so we don't drop the ACK sending end yet.
            client
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u16> = vec![];
            while let Some(next) = server.next().await {
                let (item, ticket) = next.unwrap();
                // Receive each item sent by the sink.
                items.push(u16::decode(&item));
                // Make sure to drop the ticket after processing.
                drop(ticket);
            }
            items
        });

        let (send_result, recv_result) = tokio::join!(send_fut, recv_fut);
        assert!(send_result.is_ok());
        assert_eq!(
            recv_result.unwrap(),
            (0..u16::MAX).into_iter().rev().collect::<Vec<u16>>()
        );
    }

    // TODO: Test overflows kill the connection.
}
