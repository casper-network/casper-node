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
    collections::{HashSet, VecDeque},
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use bimap::BiMap;
use bytes::{Buf, Bytes, BytesMut};
use futures::Stream;
use portable_atomic::AtomicU128;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc::{
        error::{TryRecvError, TrySendError},
        Receiver, Sender,
    },
};

use crate::{
    header::Header,
    protocol::{
        payload_is_multi_frame, CompletedRead, FrameIter, JulietProtocol, LocalProtocolViolation,
        OutgoingFrame, OutgoingMessage,
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
        data.map(|b| b.len()).unwrap_or(0))
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
    receiver: Receiver<QueuedItem>,
    /// Mapping for outgoing requests, mapping internal IDs to public ones.
    request_map: BiMap<IoId, (ChannelId, Id)>,
    /// A set of channels whose wait queues should be checked again for data to send.
    dirty_channels: HashSet<ChannelId>,

    /// Shared data across handles and [`IoCore`].
    shared: Arc<IoShared<N>>,
}

/// Shared data between an [`IoCore`] handle and the core itself.
///
/// Its core functionality is to determine whether or not there is room to buffer additional
/// messages.
struct IoShared<const N: usize> {
    /// Number of requests already buffered per channel.
    requests_buffered: [AtomicUsize; N],
    /// Maximum allowed number of requests to buffer per channel.
    requests_limit: [usize; N],
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
    /// The connection was cleanly shut down without any error.
    ///
    /// Clients must no longer call [`IoCore::next_event`] after receiving this and drop the
    /// [`IoCore`] instead, likely causing the underlying transports to be closed as well.
    Closed,
}

impl IoEvent {
    /// Determine whether or not the received [`IoEvent`] is an [`IoEvent::Closed`], which indicated
    /// we should stop polling the connection.
    #[inline(always)]
    fn is_closed(&self) -> bool {
        matches!(self, IoEvent::Closed)
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
    pub async fn next_event(&mut self) -> Result<IoEvent, CoreError> {
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
                            return self.handle_completed_read(successful_read);
                        }
                    }
                }
            }

            tokio::select! {
                biased;  // We actually like the bias, avoid the randomness overhead.

                // Writing outgoing data if there is more to send.
                write_result = self.writer.write_all_buf(self.current_frame.as_mut().unwrap())
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
                        return Ok(IoEvent::Closed);
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
                            return Ok(IoEvent::Closed);
                        }
                    }

                    loop {
                        match self.receiver.try_recv() {
                            Ok(item) => {
                                self.handle_incoming_item(item)?;
                            }
                            Err(TryRecvError::Disconnected) => {
                                // While processing incoming items, the last handle was closed.
                                return Ok(IoEvent::Closed);
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

        self.send_to_ready_queue(item)
    }

    /// Sends an item directly to the ready queue, causing it to be sent out eventually.
    fn send_to_ready_queue(&mut self, item: QueuedItem) -> Result<(), LocalProtocolViolation> {
        match item {
            QueuedItem::Request {
                io_id,
                channel,
                payload,
            } => {
                // "Chase" our own requests here -- if the request was still in the wait queue,
                // we can cancel it by checking if the `IoId` has been removed in the meantime.
                //
                // Note that this only cancels multi-frame requests.
                if self.request_map.contains_left(&io_id) {
                    let msg = self.juliet.create_request(channel, payload)?;
                    let id = msg.header().id();
                    self.request_map.insert(io_id, (channel, id));
                    self.ready_queue.push_back(msg.frames());
                }
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

        // Try to fetch a frame from the ready queue. If there is nothing, we are stuck for now.
        let (frame, more) = match self.ready_queue.pop_front() {
            Some(item) => item,
            None => return Ok(None),
        }
        // Queue is empty, there is no next frame.
        .next_owned(self.juliet.max_frame_size());

        // If there are more frames after this one, schedule the remainder.
        if let Some(next_frame_iter) = more {
            self.ready_queue.push_back(next_frame_iter);
        } else {
            // No additional frames, check if sending the next frame will finish a multi-frame.
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

    /// Process the wait queue, moving messages that are ready to be sent to the ready queue.
    fn process_dirty_channels(&mut self) -> Result<(), LocalProtocolViolation> {
        // TODO: process dirty channels

        // // TODO: Rewrite, factoring out functions from `handle_incoming`.

        // let active_multi_frame = &self.active_multi_frame[channel.get() as usize];
        // let wait_queue = &mut self.wait_queue[channel.get() as usize];
        // for _ in 0..(wait_queue.len()) {
        //     // Note: We do not use `drain` here, since we want to modify in-place. `retain` is also
        //     //       not used, since it does not allow taking out items by-value. An alternative
        //     //       might be sorting the list and splitting off the candidates instead.
        //     let item = wait_queue
        //         .pop_front()
        //         .expect("did not expect to run out of items");

        //     if item_is_ready(channel, &item, &self.juliet, active_multi_frame) {
        //         match item {
        //             QueuedItem::Request { payload } => {
        //                 let msg = self.juliet.create_request(channel, payload)?;
        //                 self.ready_queue.push_back(msg.frames());
        //             }
        //             QueuedItem::Response { io_id: id, payload } => {
        //                 if let Some(msg) = self.juliet.create_response(channel, id, payload)? {
        //                     self.ready_queue.push_back(msg.frames());
        //                 }
        //             }
        //             QueuedItem::RequestCancellation { io_id: id } => {
        //                 if let Some(msg) = self.juliet.cancel_request(channel, id)? {
        //                     self.ready_queue.push_back(msg.frames());
        //                 }
        //             }
        //             QueuedItem::ResponseCancellation { io_id: id } => {
        //                 if let Some(msg) = self.juliet.cancel_response(channel, id)? {
        //                     self.ready_queue.push_back(msg.frames());
        //                 }
        //             }
        //             QueuedItem::Error { id, payload } => {
        //                 let msg = self.juliet.custom_error(channel, id, payload)?;
        //                 // Errors go into the front.
        //                 self.ready_queue.push_front(msg.frames());
        //             }
        //         }
        //     } else {
        //         wait_queue.push_back(item);
        //     }
        // }

        Ok(())
    }

    fn into_stream(self) -> impl Stream<Item = Result<IoEvent, CoreError>> {
        futures::stream::unfold(Some(self), |state| async {
            let mut this = state?;
            let rv = this.next_event().await;

            // Check if this was the last event. We shut down on close or any error.
            if rv.as_ref().map(IoEvent::is_closed).unwrap_or(true) {
                Some((rv, None))
            } else {
                Some((rv, Some(this)))
            }
        })
    }
}

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

    let mut active_multi_frame = active_multi_frame[channel.get() as usize];

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

struct IoHandle<const N: usize> {
    shared: Arc<IoShared<N>>,
    /// Sender for queue items.
    sender: Sender<QueuedItem>,
    next_io_id: Arc<AtomicU128>,
}

#[derive(Debug, Error)]
enum EnqueueError {
    /// The IO core was shut down, there is no connection anymore to send through.
    #[error("IO closed")]
    Closed(Option<Bytes>),
    /// The request limit was hit, try again.
    #[error("request limit hit")]
    RequestLimitHit(Option<Bytes>),
    /// API violation.
    #[error("local protocol violation during enqueueing")]
    LocalProtocolViolation(#[from] LocalProtocolViolation),
}

impl EnqueueError {
    #[inline(always)]
    fn from_failed_send(err: TrySendError<QueuedItem>) -> Self {
        match err {
            // Note: The `Full` state should never happen unless our queue sizing is incorrect, we
            //       sweep this under the rug here.
            TrySendError::Full(item) => EnqueueError::RequestLimitHit(item.into_payload()),
            TrySendError::Closed(item) => EnqueueError::Closed(item.into_payload()),
        }
    }
}

impl<const N: usize> IoHandle<N> {
    fn enqueue_request(
        &mut self,
        channel: ChannelId,
        payload: Option<Bytes>,
    ) -> Result<IoId, EnqueueError> {
        bounds_check::<N>(channel)?;

        let count = &self.shared.requests_buffered[channel.get() as usize];
        let limit = self.shared.requests_limit[channel.get() as usize];

        // TODO: relax ordering from `SeqCst`.
        match count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if current < limit {
                Some(current + 1)
            } else {
                None
            }
        }) {
            Ok(_prev) => {
                // Does not overflow before at least 10^18 zettabytes have been sent.
                let io_id = IoId(self.next_io_id.fetch_add(1, Ordering::Relaxed));

                self.sender
                    .try_send(QueuedItem::Request {
                        io_id,
                        channel,
                        payload,
                    })
                    .map_err(EnqueueError::from_failed_send)?;
                Ok(io_id)
            }
            Err(_prev) => Err(EnqueueError::RequestLimitHit(payload)),
        }
    }

    fn enqueue_response(
        &self,
        channel: ChannelId,
        id: Id,
        payload: Option<Bytes>,
    ) -> Result<(), EnqueueError> {
        self.sender
            .try_send(QueuedItem::Response {
                channel,
                id,
                payload,
            })
            .map_err(EnqueueError::from_failed_send)
    }

    fn enqueue_request_cancellation(
        &self,
        channel: ChannelId,
        io_id: IoId,
    ) -> Result<(), EnqueueError> {
        bounds_check::<N>(channel)?;

        self.sender
            .try_send(QueuedItem::RequestCancellation { io_id })
            .map_err(EnqueueError::from_failed_send)
    }

    fn enqueue_response_cancellation(
        &self,
        channel: ChannelId,
        id: Id,
    ) -> Result<(), EnqueueError> {
        bounds_check::<N>(channel)?;

        self.sender
            .try_send(QueuedItem::ResponseCancellation { id, channel })
            .map_err(EnqueueError::from_failed_send)
    }

    fn enqueue_error(
        &self,
        channel: ChannelId,
        id: Id,
        payload: Bytes,
    ) -> Result<(), EnqueueError> {
        self.sender
            .try_send(QueuedItem::Error {
                id,
                channel,
                payload,
            })
            .map_err(EnqueueError::from_failed_send)
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

#[inline(always)]
fn bounds_check<const N: usize>(channel: ChannelId) -> Result<(), LocalProtocolViolation> {
    if channel.get() as usize >= N {
        Err(LocalProtocolViolation::InvalidChannel(channel))
    } else {
        Ok(())
    }
}