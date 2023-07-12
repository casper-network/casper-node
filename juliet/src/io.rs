//! `juliet` IO layer
//!
//! The IO layer combines a lower-level transport like a TCP Stream with the
//! [`JulietProtocol`](crate::juliet::JulietProtocol) protocol implementation and some memory
//! buffers to provide a working high-level transport for juliet messages. It allows users of this
//! layer to send messages across over multiple channels, without having to worry about frame
//! multiplexing or request limits.
//!
//! See [`IoCore`] for more information about how to use this module.

use std::{
    collections::{BTreeSet, VecDeque},
    io,
    sync::{atomic::Ordering, Arc},
};

use bimap::BiMap;
use bytes::{Buf, Bytes, BytesMut};
use futures::Stream;
use portable_atomic::AtomicU128;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{
        mpsc::{self, error::TryRecvError, UnboundedReceiver, UnboundedSender},
        AcquireError, OwnedSemaphorePermit, Semaphore, TryAcquireError,
    },
};

use crate::{
    header::Header,
    protocol::{
        payload_is_multi_frame, CompletedRead, FrameIter, JulietProtocol, LocalProtocolViolation,
        OutgoingFrame, OutgoingMessage, ProtocolBuilder,
    },
    ChannelId, Id, Outcome,
};

/// An item in the outgoing queue.
///
/// Requests are not transformed into messages in the queue to conserve limited request ID space.
#[derive(Debug)]
enum QueuedItem {
    /// An outgoing request.
    Request {
        /// Channel to send it out on.
        channel: ChannelId,
        /// [`IoId`] mapped to the request.
        io_id: IoId,
        /// The requests payload.
        payload: Option<Bytes>,
        /// The semaphore permit for the request.
        permit: OwnedSemaphorePermit,
    },
    /// Cancellation of one of our own requests.
    RequestCancellation {
        /// [`IoId`] mapped to the request that should be cancelled.
        io_id: IoId,
    },
    /// Outgoing response to a received request.
    Response {
        /// Channel the original request was received on.
        channel: ChannelId,
        /// Id of the original request.
        id: Id,
        /// Payload to send along with the response.
        payload: Option<Bytes>,
    },
    /// A cancellation response.
    ResponseCancellation {
        /// Channel the original request was received on.
        channel: ChannelId,
        /// Id of the original request.
        id: Id,
    },
    /// An error.
    Error {
        /// Channel to send error on.
        channel: ChannelId,
        /// Id to send with error.
        id: Id,
        /// Error payload.
        payload: Bytes,
    },
}

impl QueuedItem {
    /// Retrieves the payload from the queued item.
    fn into_payload(self) -> Option<Bytes> {
        match self {
            QueuedItem::Request { payload, .. } => payload,
            QueuedItem::Response { payload, .. } => payload,
            QueuedItem::RequestCancellation { .. } => None,
            QueuedItem::ResponseCancellation { .. } => None,
            QueuedItem::Error { payload, .. } => Some(payload),
        }
    }
}

/// [`IoCore`] error.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Failed to read from underlying reader.
    #[error("read failed")]
    ReadFailed(#[source] io::Error),
    /// Failed to write using underlying writer.
    #[error("write failed")]
    WriteFailed(#[source] io::Error),
    /// Remote peer disconnecting due to error.
    #[error("remote peer sent error [channel {}/id {}]: {} (payload: {} bytes)",
        header.channel(),
        header.id(),
        header.error_kind(),
        data.as_ref().map(|b| b.len()).unwrap_or(0))
    ]
    RemoteReportedError { header: Header, data: Option<Bytes> },
    /// The remote peer violated the protocol and has been sent an error.
    #[error("error sent to peer")]
    RemoteProtocolViolation(OutgoingFrame),
    #[error("local protocol violation")]
    /// Local protocol violation - caller violated the crate's API.
    LocalProtocolViolation(#[from] LocalProtocolViolation),
    /// Internal error.
    ///
    /// An error occured that should be impossible, this is indicative of a bug in this library.
    #[error("internal consistency error: {0}")]
    InternalError(&'static str),
}

/// An IO layer request ID.
///
/// Request layer IO IDs are unique across the program per request that originated from the local
/// endpoint. They are used to allow for buffering large numbers of items without exhausting the
/// pool of protocol level request IDs, which are limited to `u16`s.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IoId(u128);

/// IO layer for the juliet protocol.
///
/// The central structure for the IO layer built on top the juliet protocol, once instance per
/// connection. It manages incoming (`R`) and outgoing (`W`) transports, as well as a queue for
/// items to be sent.
///
/// Once instantiated, a continuously polling of [`IoCore::next_event`] is expected.
pub struct IoCore<const N: usize, R, W> {
    /// The actual protocol state.
    juliet: JulietProtocol<N>,

    /// Underlying transport, reader.
    reader: R,
    /// Underlying transport, writer.
    writer: W,
    /// Read buffer for incoming data.
    buffer: BytesMut,
    /// How many more bytes are required until the next parse.
    ///
    /// Used to ensure we don't attempt to parse too often.
    next_parse_at: usize,
    /// Whether or not we are shutting down due to an error.
    shutting_down_due_to_err: bool,

    /// The frame in the process of being sent, which may be partially transferred already.
    current_frame: Option<OutgoingFrame>,
    /// The headers of active current multi-frame transfers.
    active_multi_frame: [Option<Header>; N],
    /// Frames waiting to be sent.
    ready_queue: VecDeque<FrameIter>,
    /// Messages that are not yet ready to be sent.
    wait_queue: [VecDeque<QueuedItem>; N],
    /// Receiver for new messages to be queued.
    receiver: UnboundedReceiver<QueuedItem>,
    /// Mapping for outgoing requests, mapping internal IDs to public ones.
    request_map: BiMap<IoId, (ChannelId, Id)>,
    /// A set of channels whose wait queues should be checked again for data to send.
    dirty_channels: BTreeSet<ChannelId>,

    /// Shared data across handles and [`IoCore`].
    shared: Arc<IoShared<N>>,
}

/// Shared data between a handles and the core itself.
#[derive(Debug)]
#[repr(transparent)]
struct IoShared<const N: usize> {
    /// Tracks how many requests are in the wait queue.
    ///
    /// Tickets are freed once the item is in the wait queue, thus the semaphore permit count
    /// controls how many requests can be buffered in addition to those already permitted due to the
    /// protocol.
    ///
    /// The maximum number of available tickets must be >= 1 for the IO layer to function.
    buffered_requests: [Arc<Semaphore>; N],
}

/// Events produced by the IO layer.
#[derive(Debug)]
#[must_use]
pub enum IoEvent {
    /// A new request has been received.
    ///
    /// Eventually a received request must be handled by one of the following:
    ///
    /// * A response sent (through [`IoHandle::enqueue_response`]).
    /// * A response cancellation sent (through [`IoHandle::enqueue_response_cancellation`]).
    /// * The connection being closed, either regularly or due to an error, on either side.
    /// * The reception of an [`IoEvent::RequestCancellation`] with the same ID and channel.
    NewRequest {
        /// Channel the new request arrived on.
        channel: ChannelId,
        /// Request ID (set by peer).
        id: Id,
        /// The payload provided with the request.
        payload: Option<Bytes>,
    },
    /// A received request has been cancelled.
    RequestCancelled {
        /// Channel the original request arrived on.
        channel: ChannelId,
        /// Request ID (set by peer).
        id: Id,
    },
    /// A response has been received.
    ///
    /// For every [`IoId`] there will eventually be exactly either one [`IoEvent::ReceivedResponse`]
    /// or [`IoEvent::ReceivedCancellationResponse`], unless the connection is shutdown beforehand.
    ReceivedResponse {
        /// The local request ID for which the response was sent.
        io_id: IoId,
        /// The payload of the response.
        payload: Option<Bytes>,
    },
    /// A response cancellation has been received.
    ///
    /// Indicates the peer is not going to answer the request.
    ///
    /// For every [`IoId`] there will eventually be exactly either one [`IoEvent::ReceivedResponse`]
    /// or [`IoEvent::ReceivedCancellationResponse`], unless the connection is shutdown beforehand.
    ReceivedCancellationResponse {
        /// The local request ID which will not be answered.
        io_id: IoId,
    },
}

/// A builder for the [`IoCore`].
#[derive(Debug)]
pub struct IoCoreBuilder<const N: usize> {
    /// The builder for the underlying protocol.
    protocol: ProtocolBuilder<N>,
    /// Number of additional requests to buffer, per channel.
    buffer_size: [usize; N],
}

impl<const N: usize> IoCoreBuilder<N> {
    /// Creates a new builder for an [`IoCore`].
    #[inline]
    pub fn new(protocol: ProtocolBuilder<N>) -> Self {
        Self {
            protocol,
            buffer_size: [1; N],
        }
    }

    /// Sets the wait queue buffer size for a given channel.
    ///
    /// # Panics
    ///
    /// Will panic if given an invalid channel or a size less than one.
    pub fn buffer_size(mut self, channel: ChannelId, size: usize) -> Self {
        assert!(size > 0, "cannot have a memory buffer size of zero");

        self.buffer_size[channel.get() as usize] = size;

        self
    }

    /// Builds a new [`IoCore`] with a single request handle.
    pub fn build<R, W>(&self, reader: R, writer: W) -> (IoCore<N, R, W>, RequestHandle<N>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let shared = Arc::new(IoShared {
            buffered_requests: array_init::map_array_init(&self.buffer_size, |&sz| {
                Arc::new(Semaphore::new(sz))
            }),
        });

        let core = IoCore {
            juliet: self.protocol.build(),
            reader,
            writer,
            buffer: BytesMut::new(),
            next_parse_at: 0,
            shutting_down_due_to_err: false,
            current_frame: None,
            active_multi_frame: [Default::default(); N],
            ready_queue: Default::default(),
            wait_queue: array_init::array_init(|_| Default::default()),
            receiver,
            request_map: Default::default(),
            dirty_channels: Default::default(),
            shared: shared.clone(),
        };

        let handle = RequestHandle {
            shared,
            sender,
            next_io_id: Default::default(),
        };

        (core, handle)
    }
}

impl<const N: usize, R, W> IoCore<N, R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Retrieve the next event.
    ///
    /// This is the central loop of the IO layer. It polls all underlying transports and reads/write
    /// if data is available, until enough processing has been done to produce an [`IoEvent`]. Thus
    /// any application using the IO layer should loop over calling this function, or call
    /// `[IoCore::into_stream]` to process it using the standard futures stream interface.
    ///
    /// Polling of this function should continue until `Err(_)` or `Ok(None)` is returned.
    pub async fn next_event(&mut self) -> Result<Option<IoEvent>, CoreError> {
        loop {
            self.process_dirty_channels()?;

            if self.next_parse_at <= self.buffer.remaining() {
                // Simplify reasoning about this code.
                self.next_parse_at = 0;

                loop {
                    match self.juliet.process_incoming(&mut self.buffer) {
                        Outcome::Incomplete(n) => {
                            // Simply reset how many bytes we need until the next parse.
                            self.next_parse_at = self.buffer.remaining() + n.get() as usize;
                            break;
                        }
                        Outcome::Fatal(err_msg) => {
                            // The remote messed up, begin shutting down due to an error.
                            self.inject_error(err_msg);

                            // Stop processing incoming data.
                            break;
                        }
                        Outcome::Success(successful_read) => {
                            // Check if we have produced an event.
                            return self.handle_completed_read(successful_read).map(Some);
                        }
                    }
                }
            }

            // TODO: Can we find something more elegant than this abomination?
            #[inline(always)]
            async fn write_all_buf_if_some<W: AsyncWrite + Unpin>(
                writer: &mut W,
                buf: Option<&mut impl Buf>,
            ) -> Result<(), io::Error> {
                if let Some(buf) = buf {
                    writer.write_all_buf(buf).await
                } else {
                    Ok(())
                }
            }

            tokio::select! {
                biased;  // We actually like the bias, avoid the randomness overhead.

                write_result = write_all_buf_if_some(&mut self.writer, self.current_frame.as_mut())
                , if self.current_frame.is_some() => {

                    write_result.map_err(CoreError::WriteFailed)?;

                    // If we just finished sending an error, it's time to exit.
                    let frame_sent = self.current_frame.take().unwrap();
                    if frame_sent.header().is_error() {
                        // We finished sending an error frame, time to exit.
                        return Err(CoreError::RemoteProtocolViolation(frame_sent));
                    }

                    // Otherwise prepare the next frame.
                    self.current_frame = self.ready_next_frame()?;
                }

                // Reading incoming data.
                read_result = read_until_bytesmut(&mut self.reader, &mut self.buffer, self.next_parse_at), if !self.shutting_down_due_to_err => {
                    // Our read function will not return before `read_until_bytesmut` has completed.
                    let bytes_read = read_result.map_err(CoreError::ReadFailed)?;

                    if bytes_read == 0 {
                        // Remote peer hung up.
                        return Ok(None);
                    }

                    // Fall through to start of loop, which parses data read.
                }

                // Processing locally queued things.
                incoming = self.receiver.recv(), if !self.shutting_down_due_to_err => {
                    match incoming {
                        Some(item) => {
                            self.handle_incoming_item(item)?;
                        }
                        None => {
                            // If the receiver was closed it means that we locally shut down the
                            // connection.
                            return Ok(None);
                        }
                    }

                    loop {
                        match self.receiver.try_recv() {
                            Ok(item) => {
                                self.handle_incoming_item(item)?;
                            }
                            Err(TryRecvError::Disconnected) => {
                                // While processing incoming items, the last handle was closed.
                                return Ok(None);
                            }
                            Err(TryRecvError::Empty) => {
                                // Everything processed.
                                break
                            }
                        }
                    }
                }
            }
        }
    }

    /// Ensures the next message sent is an error message.
    ///
    /// Clears all buffers related to sending and closes the local incoming channel.
    fn inject_error(&mut self, err_msg: OutgoingMessage) {
        // Stop accepting any new local data.
        self.receiver.close();

        // Set the error state.
        self.shutting_down_due_to_err = true;

        // We do not continue parsing, ever again.
        self.next_parse_at = usize::MAX;

        // Clear queues and data structures that are no longer needed.
        self.buffer.clear();
        self.ready_queue.clear();
        self.request_map.clear();
        for queue in &mut self.wait_queue {
            queue.clear();
        }

        // Ensure the error message is the next frame sent.
        self.ready_queue.push_front(err_msg.frames());
    }

    /// Processes a completed read into a potential event.
    fn handle_completed_read(
        &mut self,
        completed_read: CompletedRead,
    ) -> Result<IoEvent, CoreError> {
        match completed_read {
            CompletedRead::ErrorReceived { header, data } => {
                // We've received an error from the peer, they will be closing the connection.
                Err(CoreError::RemoteReportedError { header, data })
            }
            CompletedRead::NewRequest {
                channel,
                id,
                payload,
            } => {
                // Requests have their id passed through, since they are not given an `IoId`.
                Ok(IoEvent::NewRequest {
                    channel,
                    id,
                    payload,
                })
            }
            CompletedRead::RequestCancellation { channel, id } => {
                Ok(IoEvent::RequestCancelled { channel, id })
            }

            // It is not our job to ensure we do not receive duplicate responses or cancellations;
            // this is taken care of by `JulietProtocol`.
            CompletedRead::ReceivedResponse {
                channel,
                id,
                payload,
            } => self
                .request_map
                .remove_by_right(&(channel, id))
                .ok_or(CoreError::InternalError(
                    "juliet protocol should have dropped response after cancellation",
                ))
                .map(move |(io_id, _)| IoEvent::ReceivedResponse { io_id, payload }),
            CompletedRead::ResponseCancellation { channel, id } => {
                // Responses are mapped to the respective `IoId`.
                self.request_map
                    .remove_by_right(&(channel, id))
                    .ok_or(CoreError::InternalError(
                        "juliet protocol should not have allowed fictitious response through",
                    ))
                    .map(|(io_id, _)| IoEvent::ReceivedCancellationResponse { io_id })
            }
        }
    }

    /// Handles a new item to send out that arrived through the incoming channel.
    fn handle_incoming_item(&mut self, item: QueuedItem) -> Result<(), LocalProtocolViolation> {
        // Check if the item is sendable immediately.
        if let Some(channel) = item_should_wait(&item, &self.juliet, &self.active_multi_frame) {
            self.wait_queue[channel.get() as usize].push_back(item);
            return Ok(());
        }

        self.send_to_ready_queue(item, false)
    }

    /// Sends an item directly to the ready queue, causing it to be sent out eventually.
    ///
    /// `item` is passed as a mutable reference for compatibility with functions like `retain_mut`,
    /// but will be left with all payloads removed, thus should likely not be reused.
    fn send_to_ready_queue(
        &mut self,
        item: QueuedItem,
        check_for_cancellation: bool,
    ) -> Result<(), LocalProtocolViolation> {
        match item {
            QueuedItem::Request {
                io_id,
                channel,
                payload,
                permit,
            } => {
                // "Chase" our own requests here -- if the request was still in the wait queue,
                // we can cancel it by checking if the `IoId` has been removed in the meantime.
                //
                // Note that this only cancels multi-frame requests.
                if check_for_cancellation && !self.request_map.contains_left(&io_id) {
                    // We just ignore the request, as it has been cancelled in the meantime.
                } else {
                    let msg = self.juliet.create_request(channel, payload)?;
                    let id = msg.header().id();
                    self.request_map.insert(io_id, (channel, id));
                    self.ready_queue.push_back(msg.frames());
                }

                drop(permit);
            }
            QueuedItem::RequestCancellation { io_id } => {
                if let Some((_, (channel, id))) = self.request_map.remove_by_left(&io_id) {
                    if let Some(msg) = self.juliet.cancel_request(channel, id)? {
                        self.ready_queue.push_back(msg.frames());
                    }
                } else {
                    // Already cancelled or answered by peer - no need to do anything.
                }
            }

            // `juliet` already tracks whether we still need to send the cancellation.
            // Unlike requests, we do not attempt to fish responses out of the queue,
            // cancelling a response after it has been created should be rare.
            QueuedItem::Response {
                id,
                channel,
                payload,
            } => {
                if let Some(msg) = self.juliet.create_response(channel, id, payload)? {
                    self.ready_queue.push_back(msg.frames())
                }
            }
            QueuedItem::ResponseCancellation { id, channel } => {
                if let Some(msg) = self.juliet.cancel_response(channel, id)? {
                    self.ready_queue.push_back(msg.frames());
                }
            }

            // Errors go straight to the front of the line.
            QueuedItem::Error {
                id,
                channel,
                payload,
            } => {
                let err_msg = self.juliet.custom_error(channel, id, payload)?;
                self.inject_error(err_msg);
            }
        }

        Ok(())
    }

    /// Clears a potentially finished frame and returns the next frame to send.
    ///
    /// Returns `None` if no frames are ready to be sent. Note that there may be frames waiting
    /// that cannot be sent due them being multi-frame messages when there already is a multi-frame
    /// message in progress, or request limits are being hit.
    fn ready_next_frame(&mut self) -> Result<Option<OutgoingFrame>, LocalProtocolViolation> {
        debug_assert!(self.current_frame.is_none()); // Must be guaranteed by caller.

        // Try to fetch a frame from the ready queue. If there is nothing, we are stuck until the
        // next time the wait queue is processed or new data arrives.
        let (frame, additional_frames) = match self.ready_queue.pop_front() {
            Some(item) => item,
            None => return Ok(None),
        }
        .next_owned(self.juliet.max_frame_size());

        // If there are more frames after this one, schedule the remainder.
        if let Some(next_frame_iter) = additional_frames {
            self.ready_queue.push_back(next_frame_iter);
        } else {
            // No additional frames. Check if sending the next frame finishes a multi-frame message.
            let about_to_finish = frame.header();
            if let Some(ref active_multi) =
                self.active_multi_frame[about_to_finish.channel().get() as usize]
            {
                if about_to_finish == *active_multi {
                    // Once the scheduled frame is processed, we will finished the multi-frame
                    // transfer, so we can allow for the next multi-frame transfer to be scheduled.
                    self.active_multi_frame[about_to_finish.channel().get() as usize] = None;

                    // There is a chance another multi-frame messages became ready now.
                    self.dirty_channels.insert(about_to_finish.channel());
                }
            }
        }

        Ok(Some(frame))
    }

    /// Process the wait queue of all channels marked dirty, promoting messages that are ready to be
    /// sent to the ready queue.
    fn process_dirty_channels(&mut self) -> Result<(), CoreError> {
        while let Some(channel) = self.dirty_channels.pop_first() {
            let wait_queue_len = self.wait_queue[channel.get() as usize].len();

            // The code below is not as bad it looks complexity wise, anticipating two common cases:
            //
            // 1. A multi-frame read has finished, with capacity for requests to spare. Only
            //    multi-frame requests will be waiting in the wait queue, so we will likely pop the
            //    first item, only scanning the rest once.
            // 2. One or more requests finished, so we also have a high chance of picking the first
            //    few requests out of the queue.

            for _ in 0..(wait_queue_len) {
                let item = self.wait_queue[channel.get() as usize].pop_front().ok_or(
                    CoreError::InternalError("did not expect wait_queue to disappear"),
                )?;

                if item_should_wait(&item, &self.juliet, &self.active_multi_frame).is_some() {
                    // Put it right back into the queue.
                    self.wait_queue[channel.get() as usize].push_back(item);
                } else {
                    self.send_to_ready_queue(item, true)?;
                }
            }
        }

        Ok(())
    }
}

/// Determines whether an item is ready to be moved from the wait queue from the ready queue.
fn item_should_wait<const N: usize>(
    item: &QueuedItem,
    juliet: &JulietProtocol<N>,
    active_multi_frame: &[Option<Header>; N],
) -> Option<ChannelId> {
    let (payload, channel) = match item {
        QueuedItem::Request {
            channel, payload, ..
        } => {
            // Check if we cannot schedule due to the message exceeding the request limit.
            if !juliet
                .allowed_to_send_request(*channel)
                .expect("should not be called with invalid channel")
            {
                return Some(*channel);
            }

            (payload, channel)
        }
        QueuedItem::Response {
            channel, payload, ..
        } => (payload, channel),

        // Other messages are always ready.
        QueuedItem::RequestCancellation { .. }
        | QueuedItem::ResponseCancellation { .. }
        | QueuedItem::Error { .. } => return None,
    };

    let active_multi_frame = active_multi_frame[channel.get() as usize];

    // Check if we cannot schedule due to the message being multi-frame and there being a
    // multi-frame send in progress:
    if active_multi_frame.is_some() {
        if let Some(payload) = payload {
            if payload_is_multi_frame(juliet.max_frame_size(), payload.len()) {
                return Some(*channel);
            }
        }
    }

    // Otherwise, this should be a legitimate add to the run queue.
    None
}

/// A handle to the input queue to the [`IoCore`] that allows sending requests and responses.
///
/// The handle is roughly three pointers in size and can be cloned at will. Dropping the last handle
/// will cause the [`IoCore`] to shutdown and close the connection.
#[derive(Clone, Debug)]
pub struct RequestHandle<const N: usize> {
    /// Shared portion of the [`IoCore`], required for backpressuring onto clients.
    shared: Arc<IoShared<N>>,
    /// Sender for queue items.
    sender: UnboundedSender<QueuedItem>,
    /// The next generation [`IoId`].
    ///
    /// IoIDs are just generated sequentially until they run out (which at 1 billion at second takes
    /// roughly 10^22 years).
    next_io_id: Arc<AtomicU128>,
}

#[derive(Clone, Debug)]
#[repr(transparent)]
pub struct Handle {
    /// Sender for queue items.
    sender: UnboundedSender<QueuedItem>,
}

/// An error that can occur while attempting to enqueue an item.
#[derive(Debug, Error)]
pub enum EnqueueError {
    /// The IO core was shut down, there is no connection anymore to send through.
    #[error("IO closed")]
    Closed(Option<Bytes>),
    /// The request limit for locally buffered requests was hit, try again.
    #[error("request limit hit")]
    BufferLimitHit(Option<Bytes>),
    /// Violation of local invariants, this is likely a bug in this library or the calling code.
    #[error("local protocol violation during enqueueing")]
    LocalProtocolViolation(#[from] LocalProtocolViolation),
}

#[derive(Debug)]
pub struct RequestTicket {
    channel: ChannelId,
    permit: OwnedSemaphorePermit,
    io_id: IoId,
}

pub enum ReservationError {
    NoBufferSpaceAvailable,
    Closed,
}

impl<const N: usize> RequestHandle<N> {
    /// Attempts to reserve a new request ticket.
    #[inline]
    pub fn try_reserve_request(
        &self,
        channel: ChannelId,
    ) -> Result<RequestTicket, ReservationError> {
        match self.shared.buffered_requests[channel.get() as usize]
            .clone()
            .try_acquire_owned()
        {
            Ok(permit) => Ok(RequestTicket {
                channel,
                permit,
                io_id: IoId(self.next_io_id.fetch_add(1, Ordering::Relaxed)),
            }),

            Err(TryAcquireError::Closed) => Err(ReservationError::Closed),
            Err(TryAcquireError::NoPermits) => Err(ReservationError::NoBufferSpaceAvailable),
        }
    }

    /// Reserves a new request ticket.
    #[inline]
    pub async fn reserve_request(&self, channel: ChannelId) -> Option<RequestTicket> {
        self.shared.buffered_requests[channel.get() as usize]
            .clone()
            .acquire_owned()
            .await
            .map(|permit| RequestTicket {
                channel,
                permit,
                io_id: IoId(self.next_io_id.fetch_add(1, Ordering::Relaxed)),
            })
            .ok()
    }

    #[inline(always)]
    pub fn downgrade(self) -> Handle {
        Handle {
            sender: self.sender,
        }
    }
}

impl Handle {
    /// Enqueues a new request.
    ///
    /// Returns an [`IoId`] that can be used to refer to the request if successful. Returns the
    /// payload as an error if the underlying IO layer has been closed.
    #[inline]
    pub fn enqueue_request(
        &mut self,
        RequestTicket {
            channel,
            permit,
            io_id,
        }: RequestTicket,
        payload: Option<Bytes>,
    ) -> Result<IoId, Option<Bytes>> {
        // TODO: Panic if given semaphore ticket from wrong instance?

        self.sender
            .send(QueuedItem::Request {
                io_id,
                channel,
                payload,
                permit,
            })
            .map_err(|send_err| send_err.0.into_payload())?;

        Ok(io_id)
    }

    /// Enqueues a response to an existing request.
    ///
    /// Callers are supposed to send only one response or cancellation per incoming request.
    pub fn enqueue_response(
        &self,
        channel: ChannelId,
        id: Id,
        payload: Option<Bytes>,
    ) -> Result<(), EnqueueError> {
        self.sender
            .send(QueuedItem::Response {
                channel,
                id,
                payload,
            })
            .map_err(|send_err| EnqueueError::Closed(send_err.0.into_payload()))
    }

    /// Enqueues a cancellation to an existing outgoing request.
    ///
    /// If the request has already been answered or cancelled, the enqueue cancellation will
    /// ultimately have no effect.
    pub fn enqueue_request_cancellation(&self, io_id: IoId) -> Result<(), EnqueueError> {
        self.sender
            .send(QueuedItem::RequestCancellation { io_id })
            .map_err(|send_err| EnqueueError::Closed(send_err.0.into_payload()))
    }

    /// Enqueues a cancellation as a response to a received request.
    ///
    /// Callers are supposed to send only one response or cancellation per incoming request.
    pub fn enqueue_response_cancellation(
        &self,
        channel: ChannelId,
        id: Id,
    ) -> Result<(), EnqueueError> {
        self.sender
            .send(QueuedItem::ResponseCancellation { id, channel })
            .map_err(|send_err| EnqueueError::Closed(send_err.0.into_payload()))
    }

    /// Enqueus an error.
    ///
    /// Enqueuing an error causes the [`IoCore`] to begin shutting down immediately, only making an
    /// effort to finish sending the error before doing so.
    pub fn enqueue_error(
        &self,
        channel: ChannelId,
        id: Id,
        payload: Bytes,
    ) -> Result<(), EnqueueError> {
        self.sender
            .send(QueuedItem::Error {
                id,
                channel,
                payload,
            })
            .map_err(|send_err| EnqueueError::Closed(send_err.0.into_payload()))
    }
}

/// Read bytes into a buffer.
///
/// Similar to [`AsyncReadExt::read_buf`], except it performs multiple read calls until at least
/// `target` bytes are in `buf`.
///
/// Will automatically retry if an [`io::ErrorKind::Interrupted`] is returned.
///
/// # Cancellation safety
///
/// This function is cancellation safe in the same way that [`AsyncReadExt::read_buf`] is.
async fn read_until_bytesmut<'a, R>(
    reader: &'a mut R,
    buf: &mut BytesMut,
    target: usize,
) -> io::Result<usize>
where
    R: AsyncReadExt + Sized + Unpin,
{
    let mut bytes_read = 0;
    buf.reserve(target);

    while buf.remaining() < target {
        match reader.read_buf(buf).await {
            Ok(n) => bytes_read += n,
            Err(err) => {
                if matches!(err.kind(), io::ErrorKind::Interrupted) {
                    continue;
                }
                return Err(err);
            }
        }
    }

    Ok(bytes_read)
}
