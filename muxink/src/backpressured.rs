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
    // TODO: `Unpin` trait bounds can be
    // removed by using `map_unchecked` if
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
pub enum BackpressuredStreamError<E: std::error::Error> {
    /// Couldn't enqueue an ACK for sending on the ACK sink after it polled
    /// ready.
    #[error("Error sending ACK to sender")]
    AckSend,
    /// Error on polling the ACK sink.
    #[error("Error polling the ACK stream")]
    AckSinkPoll,
    /// Error flushing the ACK sink.
    #[error("Error flushing the ACK stream")]
    Flush,
    /// Error on the underlying stream when it is ready to yield a new item,
    /// but doing so would bring the number of in flight items over the
    /// limit imposed by the window size and therefore the sender broke the
    /// contract.
    #[error("Sender sent more items than the window size")]
    ItemOverflow,
    /// Error encountered by the underlying stream.
    #[error(transparent)]
    Stream(E),
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
{
    type Item = Result<(StreamItem, Ticket), BackpressuredStreamError<E>>;

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
                    if let Err(_) = self_mut.ack_sink.start_send_unpin(self_mut.items_processed) {
                        return Poll::Ready(Some(Err(BackpressuredStreamError::AckSend)));
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
                return Poll::Ready(Some(Ok((
                    next_item,
                    Ticket::new(self_mut.ack_sender.clone()),
                ))));
            }
            Some(Err(err)) => {
                // Return the error on the underlying stream.
                return Poll::Ready(Some(Err(BackpressuredStreamError::Stream(err))));
            }
            None => {
                // If the underlying stream is closed, the `BackpressuredStream`
                // is also considered closed. Polling the stream after this point
                // is undefined behavior.
                return Poll::Ready(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::{Infallible, TryInto},
        pin::Pin,
        sync::Arc,
        task::{Context, Poll},
    };

    use bytes::Bytes;
    use futures::{FutureExt, Sink, SinkExt, StreamExt};
    use tokio::sync::mpsc::UnboundedSender;
    use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
    use tokio_util::sync::PollSender;

    use crate::testing::{
        encoding::EncodeAndSend,
        testing_sink::{TestingSink, TestingSinkRef},
    };

    use super::{
        BackpressureError, BackpressuredSink, BackpressuredStream, BackpressuredStreamError, Ticket,
    };

    /// Window size used in tests.
    const WINDOW_SIZE: u64 = 3;

    struct CloggedAckSink {
        clogged: bool,
        /// Buffer for items when the sink is clogged.
        buffer: VecDeque<u64>,
        /// The sink ACKs are sent into.
        ack_sender: PollSender<u64>,
    }

    impl CloggedAckSink {
        fn new(ack_sender: PollSender<u64>) -> Self {
            Self {
                clogged: false,
                buffer: VecDeque::new(),
                ack_sender,
            }
        }

        fn set_clogged(&mut self, clogged: bool) {
            self.clogged = clogged;
        }
    }

    impl Sink<u64> for CloggedAckSink {
        type Error = tokio_util::sync::PollSendError<u64>;

        fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.get_mut().ack_sender.poll_ready_unpin(cx)
        }

        fn start_send(self: Pin<&mut Self>, item: u64) -> Result<(), Self::Error> {
            let self_mut = self.get_mut();
            if self_mut.clogged {
                self_mut.buffer.push_back(item);
                Ok(())
            } else {
                self_mut.ack_sender.start_send_unpin(item)
            }
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            let self_mut = self.get_mut();
            if self_mut.clogged {
                Poll::Pending
            } else {
                if let Poll::Pending = self_mut.poll_ready_unpin(cx) {
                    return Poll::Pending;
                }
                while let Some(item) = self_mut.buffer.pop_front() {
                    self_mut.ack_sender.start_send_unpin(item).unwrap();
                }
                self_mut.ack_sender.poll_flush_unpin(cx)
            }
        }

        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.get_mut().ack_sender.poll_close_unpin(cx)
        }
    }

    /// A common set of fixtures used in the backpressure tests.
    ///
    /// The fixtures represent what a server holds when dealing with a backpressured client.

    struct Fixtures {
        /// A sender for ACKs back to the client.
        ack_sender: UnboundedSender<u64>,
        /// The clients sink for requests, with no backpressure wrapper. Used for retrieving the
        /// test data in the end or setting plugged/clogged status.
        sink: Arc<TestingSink>,
        /// The properly set up backpressured sink.
        bp: BackpressuredSink<TestingSinkRef, UnboundedReceiverStream<u64>, Bytes>,
    }

    impl Fixtures {
        /// Creates a new set of fixtures.
        fn new() -> Self {
            let sink = Arc::new(TestingSink::new());
            let (ack_sender, ack_receiver) = tokio::sync::mpsc::unbounded_channel::<u64>();
            let ack_stream = UnboundedReceiverStream::new(ack_receiver);

            let bp = BackpressuredSink::new(sink.clone().into_ref(), ack_stream, WINDOW_SIZE);
            Self {
                ack_sender,
                sink,
                bp,
            }
        }
    }

    #[test]
    fn backpressured_sink_lifecycle() {
        let Fixtures {
            ack_sender,
            sink,
            mut bp,
        } = Fixtures::new();

        // The first four attempts at `window_size = 3` should succeed.
        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        bp.encode_and_send('C').now_or_never().unwrap().unwrap();
        bp.encode_and_send('D').now_or_never().unwrap().unwrap();

        // The fifth attempt will fail, due to no ACKs having been received.
        assert!(bp.encode_and_send('E').now_or_never().is_none());

        // We can now send some ACKs.
        ack_sender.send(1).unwrap();

        // Retry sending the fifth message, sixth should still block.
        bp.encode_and_send('E').now_or_never().unwrap().unwrap();
        assert!(bp.encode_and_send('F').now_or_never().is_none());

        // Send a combined ack for three messages.
        ack_sender.send(4).unwrap();

        // This allows 3 more messages to go in.
        bp.encode_and_send('F').now_or_never().unwrap().unwrap();
        bp.encode_and_send('G').now_or_never().unwrap().unwrap();
        bp.encode_and_send('H').now_or_never().unwrap().unwrap();
        assert!(bp.encode_and_send('I').now_or_never().is_none());

        // Send more ACKs to ensure we also get errors if there is capacity.
        ack_sender.send(6).unwrap();

        // We can now close the ACK stream to check if the sink errors after that.
        drop(ack_sender);

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
        let (sink, stream) = tokio::sync::mpsc::channel::<u16>(u16::MAX as usize);
        let (ack_sender, ack_receiver) = tokio::sync::mpsc::channel::<u64>(u16::MAX as usize);
        let mut sink = BackpressuredSink::new(
            PollSender::new(sink),
            ReceiverStream::new(ack_receiver),
            WINDOW_SIZE,
        );

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u16, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        // Send 4 items, using all capacity.
        for i in 0..=WINDOW_SIZE {
            sink.send(i as u16).now_or_never().unwrap().unwrap();
        }

        let mut items = VecDeque::new();
        let mut tickets = VecDeque::new();

        // Receive the items along with their tickets.
        for _ in 0..=WINDOW_SIZE {
            let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
            items.push_back(item);
            tickets.push_back(ticket);
        }
        // Make room for 2 more items.
        let _ = tickets.pop_front();
        let _ = tickets.pop_front();
        // Send the ACKs to the sink by polling the stream.
        assert!(stream.next().now_or_never().is_none());
        assert_eq!(stream.last_received, 4);
        assert_eq!(stream.items_processed, 2);
        // Send another item. Even though at this point in the stream state
        // all capacity is used, the next poll will receive an ACK for 2 items.
        assert_eq!(sink.last_request, 4);
        assert_eq!(sink.received_ack, 0);
        sink.send(4).now_or_never().unwrap().unwrap();
        // Make sure we received the ACK and we recorded the send.
        assert_eq!(sink.last_request, 5);
        assert_eq!(sink.received_ack, 2);
        assert_eq!(stream.items_processed, 2);
        // Send another item to fill up the capacity again.
        sink.send(5).now_or_never().unwrap().unwrap();
        assert_eq!(sink.last_request, 6);

        // Receive both items.
        for _ in 0..2 {
            let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
            items.push_back(item);
            tickets.push_back(ticket);
        }
        // At this point both the sink and stream should reflect the same
        // state.
        assert_eq!(sink.last_request, 6);
        assert_eq!(sink.received_ack, 2);
        assert_eq!(stream.last_received, 6);
        assert_eq!(stream.items_processed, 2);
        // Drop all tickets.
        for _ in 0..=WINDOW_SIZE {
            let _ = tickets.pop_front();
        }
        // Send the ACKs to the sink by polling the stream.
        assert!(stream.next().now_or_never().is_none());
        // Make sure the stream state reflects the sent ACKs.
        assert_eq!(stream.items_processed, 6);
        // Send another item.
        sink.send(6).now_or_never().unwrap().unwrap();
        assert_eq!(sink.received_ack, 6);
        assert_eq!(sink.last_request, 7);
        // Receive the item.
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        // At this point both the sink and stream should reflect the same
        // state.
        assert_eq!(stream.items_processed, 6);
        assert_eq!(stream.last_received, 7);
        items.push_back(item);
        tickets.push_back(ticket);

        // Send 2 items.
        sink.send(7).now_or_never().unwrap().unwrap();
        sink.send(8).now_or_never().unwrap().unwrap();
        // Receive only 1 item.
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        // The sink state should be ahead of the stream by 1 item, which is yet
        // to be yielded in a `poll_next` by the stream.
        assert_eq!(sink.last_request, 9);
        assert_eq!(sink.received_ack, 6);
        assert_eq!(stream.items_processed, 6);
        assert_eq!(stream.last_received, 8);
        items.push_back(item);
        tickets.push_back(ticket);
        // Drop a ticket.
        let _ = tickets.pop_front();
        // Receive the other item. Also send the ACK with this poll.
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        // Ensure the stream state has been updated.
        assert_eq!(stream.items_processed, 7);
        assert_eq!(stream.last_received, 9);
        items.push_back(item);
        tickets.push_back(ticket);

        // The stream should have received all of these items.
        assert_eq!(items, [0, 1, 2, 3, 4, 5, 6, 7, 8]);

        // Now send 2 more items to occupy all available capacity in the sink.
        sink.send(9).now_or_never().unwrap().unwrap();
        // The sink should have received the latest ACK with this poll, so
        // we check it against the stream one to ensure correctness.
        assert_eq!(sink.received_ack, stream.items_processed);
        sink.send(10).now_or_never().unwrap().unwrap();
        // Make sure we reached full capacity in the sink state.
        assert_eq!(sink.last_request, sink.received_ack + WINDOW_SIZE + 1);
        // Sending a new item should return `Poll::Pending`.
        assert!(sink.send(9).now_or_never().is_none());
    }

    #[test]
    fn backpressured_sink_premature_ack_kills_stream() {
        let Fixtures {
            ack_sender, mut bp, ..
        } = Fixtures::new();

        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        ack_sender.send(3).unwrap();

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
        let Fixtures {
            ack_sender, mut bp, ..
        } = Fixtures::new();

        bp.encode_and_send('A').now_or_never().unwrap().unwrap();
        bp.encode_and_send('B').now_or_never().unwrap().unwrap();
        // Out of order ACKs work.
        ack_sender.send(2).unwrap();
        ack_sender.send(1).unwrap();
        // Send 3 more items to make it 5 in total.
        bp.encode_and_send('C').now_or_never().unwrap().unwrap();
        bp.encode_and_send('D').now_or_never().unwrap().unwrap();
        bp.encode_and_send('E').now_or_never().unwrap().unwrap();
        // Send a duplicate ACK of 1, which is outside the allowed range.
        ack_sender.send(1).unwrap();

        assert!(matches!(
            bp.encode_and_send('F').now_or_never(),
            Some(Err(BackpressureError::DuplicateAck {
                ack_received: 1,
                highest: 2
            }))
        ));
    }

    #[tokio::test]
    async fn backpressured_sink_concurrent_tasks() {
        let to_send: Vec<u16> = (0..u16::MAX).into_iter().rev().collect();
        let (sink, receiver) = tokio::sync::mpsc::channel::<u16>(u16::MAX as usize);
        let (ack_sender, ack_receiver) = tokio::sync::mpsc::unbounded_channel::<u64>();
        let ack_stream = UnboundedReceiverStream::new(ack_receiver);
        let mut sink = BackpressuredSink::new(PollSender::new(sink), ack_stream, WINDOW_SIZE);

        let send_fut = tokio::spawn(async move {
            for item in to_send.iter() {
                // Try to feed each item into the sink.
                if sink.feed(*item).await.is_err() {
                    // When `feed` fails, the sink is full, so we flush it.
                    sink.flush().await.unwrap();
                    // After flushing, the sink must be able to accept new items.
                    sink.feed(*item).await.unwrap();
                }
            }
            // Close the sink here to signal the end of the stream on the other end.
            sink.close().await.unwrap();
            // Return the sink so we don't drop the ACK sending end yet.
            sink
        });

        let recv_fut = tokio::spawn(async move {
            let mut item_stream = ReceiverStream::new(receiver);
            let mut items: Vec<u16> = vec![];
            while let Some(item) = item_stream.next().await {
                // Receive each item sent by the sink.
                items.push(item);
                // Send the ACK for it.
                ack_sender.send(items.len().try_into().unwrap()).unwrap();
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
        let (sink, stream) = tokio::sync::mpsc::channel::<u16>(u16::MAX as usize);

        let (ack_sender, ack_receiver) = tokio::sync::mpsc::channel::<u64>(u16::MAX as usize);
        let mut sink: BackpressuredSink<PollSender<u16>, ReceiverStream<u64>, u16> =
            BackpressuredSink::new(
                PollSender::new(sink),
                ReceiverStream::new(ack_receiver),
                WINDOW_SIZE,
            );

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u16, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        let send_fut = tokio::spawn(async move {
            for item in to_send.iter() {
                // Try to feed each item into the sink.
                if sink.feed(*item).await.is_err() {
                    // When `feed` fails, the sink is full, so we flush it.
                    sink.flush().await.unwrap();
                    // After flushing, the sink must be able to accept new items.
                    match sink.feed(*item).await {
                        Err(BackpressureError::AckStreamClosed) => {
                            return sink;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            panic!("Error on sink send: {}", e);
                        }
                    }
                }
            }
            // Close the sink here to signal the end of the stream on the other end.
            sink.close().await.unwrap();
            // Return the sink so we don't drop the ACK sending end yet.
            sink
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u16> = vec![];
            while let Some(next) = stream.next().await {
                let (item, ticket) = next.unwrap();
                // Receive each item sent by the sink.
                items.push(item);
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

    #[tokio::test]
    async fn backpressured_stream_concurrent_tasks() {
        let to_send: Vec<u16> = (0..u16::MAX).into_iter().rev().collect();
        let (sink, stream) = tokio::sync::mpsc::channel::<u16>(u16::MAX as usize);
        let (ack_sender, mut ack_receiver) = tokio::sync::mpsc::channel::<u64>(u16::MAX as usize);

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u16, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        let send_fut = tokio::spawn(async move {
            // Try to push the limit on the backpressured stream by always keeping
            // its buffer full.
            let mut window_len = WINDOW_SIZE + 1;
            let mut last_ack = 0;
            for item in to_send.iter() {
                // If we don't have any more room left to send,
                // we look for ACKs.
                if window_len == 0 {
                    let ack = {
                        // We need at least one ACK to continue, but we may have
                        // received more, so try to read everything we've got
                        // so far.
                        let mut ack = ack_receiver.recv().await.unwrap();
                        while let Ok(new_ack) = ack_receiver.try_recv() {
                            ack = new_ack;
                        }
                        ack
                    };
                    // Update our window with the new capacity and the latest ACK.
                    window_len += ack - last_ack;
                    last_ack = ack;
                }
                // Consume window capacity and send the item.
                sink.send(*item).await.unwrap();
                window_len -= 1;
            }
            // Yield the ACK receiving end so it doesn't get dropped before the
            // stream sends everything but drop the sink so that we signal the
            // end of the stream.
            ack_receiver
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u16> = vec![];
            while let Some(next) = stream.next().await {
                let (item, ticket) = next.unwrap();
                // Receive each item sent by the sink.
                items.push(item);
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

    #[tokio::test]
    async fn backpressured_stream_hold_ticket_concurrent_tasks() {
        let to_send: Vec<u8> = (0..u8::MAX).into_iter().rev().collect();
        let (sink, stream) = tokio::sync::mpsc::channel::<u8>(u8::MAX as usize);
        let (ack_sender, mut ack_receiver) = tokio::sync::mpsc::channel::<u64>(u8::MAX as usize);

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u8, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        let send_fut = tokio::spawn(async move {
            // Try to push the limit on the backpressured stream by always keeping
            // its buffer full.
            let mut window_len = WINDOW_SIZE + 1;
            let mut last_ack = 0;
            for item in to_send.iter() {
                // If we don't have any more room left to send,
                // we look for ACKs.
                if window_len == 0 {
                    let ack = {
                        // We need at least one ACK to continue, but we may have
                        // received more, so try to read everything we've got
                        // so far.
                        let mut ack = loop {
                            let ack = ack_receiver.recv().await.unwrap();
                            if ack > last_ack {
                                break ack;
                            }
                        };
                        while let Ok(new_ack) = ack_receiver.try_recv() {
                            ack = std::cmp::max(new_ack, ack);
                        }
                        ack
                    };
                    // Update our window with the new capacity and the latest ACK.
                    window_len += ack - last_ack;
                    last_ack = ack;
                }
                // Consume window capacity and send the item.
                sink.send(*item).await.unwrap();
                window_len -= 1;
            }
            // Yield the ACK receiving end so it doesn't get dropped before the
            // stream sends everything but drop the sink so that we signal the
            // end of the stream.
            ack_receiver
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u8> = vec![];
            let mut handles = vec![];
            while let Some(next) = stream.next().await {
                let (item, ticket) = next.unwrap();
                // Receive each item sent by the sink.
                items.push(item);
                // Randomness factor.
                let factor = items.len();
                // We will have separate threads do the processing here
                // while we keep trying to receive items.
                let handle = std::thread::spawn(move || {
                    // Simulate the processing by sleeping for an
                    // arbitrary amount of time.
                    std::thread::sleep(std::time::Duration::from_micros(10 * (factor as u64 % 3)));
                    // Release the ticket to signal the end of processing.
                    // ticket.release().now_or_never().unwrap();
                    drop(ticket);
                });
                handles.push(handle);
                // If we have too many open threads, join on them and
                // drop the handles to avoid running out of resources.
                if handles.len() == WINDOW_SIZE as usize {
                    for handle in handles.drain(..) {
                        handle.join().unwrap();
                    }
                }
            }
            // Join any remaining handles.
            for handle in handles {
                handle.join().unwrap();
            }
            items
        });

        let (send_result, recv_result) = tokio::join!(send_fut, recv_fut);
        assert!(send_result.is_ok());
        assert_eq!(
            recv_result.unwrap(),
            (0..u8::MAX).into_iter().rev().collect::<Vec<u8>>()
        );
    }

    #[tokio::test]
    async fn backpressured_stream_item_overflow() {
        // `WINDOW_SIZE + 1` elements are allowed to be in flight at a single
        // point in time, so we need one more element to be able to overflow
        // the stream.
        let to_send: Vec<u16> = (0..WINDOW_SIZE as u16 + 2).into_iter().rev().collect();
        let (sink, stream) = tokio::sync::mpsc::channel::<u16>(to_send.len());
        let (ack_sender, ack_receiver) = tokio::sync::mpsc::channel::<u64>(to_send.len());

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u16, Infallible> = Ok(item);
            res
        });
        let mut stream = BackpressuredStream::new(stream, PollSender::new(ack_sender), WINDOW_SIZE);

        let send_fut = tokio::spawn(async move {
            for item in to_send.iter() {
                // Disregard the ACKs, keep sending to overflow the stream.
                if let Err(_) = sink.send(*item).await {
                    // The stream should close when we overflow it, so at some
                    // point we will receive an error when trying to send items.
                    break;
                }
            }
            ack_receiver
        });

        let recv_fut = tokio::spawn(async move {
            let mut items: Vec<u16> = vec![];
            let mut tickets: Vec<Ticket> = vec![];
            while let Some(next) = stream.next().await {
                match next {
                    Ok((item, ticket)) => {
                        // Receive each item sent by the sink.
                        items.push(item);
                        // Hold the tickets so we don't release capacity.
                        tickets.push(ticket);
                    }
                    Err(BackpressuredStreamError::ItemOverflow) => {
                        // Make sure we got this error right as the stream was
                        // about to exceed capacity.
                        assert_eq!(items.len(), WINDOW_SIZE as usize + 1);
                        return None;
                    }
                    Err(err) => {
                        panic!("Unexpected error: {}", err);
                    }
                }
            }
            Some(items)
        });

        let (send_result, recv_result) = tokio::join!(send_fut, recv_fut);
        assert!(send_result.is_ok());
        // Ensure the stream yielded an error.
        assert!(recv_result.unwrap().is_none());
    }

    #[test]
    fn backpressured_stream_ack_clogging() {
        let (sink, stream) = tokio::sync::mpsc::channel::<u8>(u8::MAX as usize);
        let (ack_sender, mut ack_receiver) = tokio::sync::mpsc::channel::<u64>(u8::MAX as usize);

        let stream = ReceiverStream::new(stream).map(|item| {
            let res: Result<u8, Infallible> = Ok(item);
            res
        });
        let mut clogged_stream = CloggedAckSink::new(PollSender::new(ack_sender));
        clogged_stream.set_clogged(true);
        let mut stream = BackpressuredStream::new(stream, clogged_stream, WINDOW_SIZE);

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
        // Drop a ticket, making room for one more item.
        let _ = tickets.pop_front();
        // Ensure no ACK was received since the sink is clogged.
        assert!(ack_receiver.recv().now_or_never().is_none());
        // Ensure polling the stream returns pending.
        assert!(stream.next().now_or_never().is_none());
        assert!(ack_receiver.recv().now_or_never().is_none());

        // Send a new item because now we should have capacity.
        sink.send(4).now_or_never().unwrap().unwrap();
        // Receive the item along with the ticket.
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        items.push_back(item);
        tickets.push_back(ticket);

        // Unclog the ACK sink. This should let 1 ACK finally flush.
        stream.ack_sink.set_clogged(false);
        // Drop another ticket.
        let _ = tickets.pop_front();
        // Send a new item with the capacity from the second ticket drop.
        sink.send(5).now_or_never().unwrap().unwrap();
        // Receive the item from the stream.
        let (item, ticket) = stream.next().now_or_never().unwrap().unwrap().unwrap();
        items.push_back(item);
        tickets.push_back(ticket);
        assert_eq!(ack_receiver.recv().now_or_never().unwrap().unwrap(), 2);
        assert!(ack_receiver.recv().now_or_never().is_none());
    }
}
