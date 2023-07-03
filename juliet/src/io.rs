//! `juliet` IO layer
//!
//! The IO layer combines a lower-level transport like a TCP Stream with the
//! [`JulietProtocol`](crate::juliet::JulietProtocol) protocol implementation and some memory buffer
//! to provide a working high-level transport for juliet messages. It allows users of this layer to
//! send messages across over multiple channels, without having to worry about frame multiplexing or
//! request limits.
//!
//! The layer is designed to run in its own task, with handles to allow sending messages in, or
//! receiving them as they arrive.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use bytes::{Bytes, BytesMut};
use futures::Stream;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc::{error::TrySendError, Receiver, Sender},
};

use crate::{
    header::Header,
    protocol::{
        CompletedRead, FrameIter, JulietProtocol, LocalProtocolViolation, OutgoingFrame,
        OutgoingMessage,
    },
    ChannelId, Id, Outcome,
};

#[derive(Debug)]
enum QueuedItem {
    Request {
        io_id: IoId,
        channel: ChannelId,
        payload: Option<Bytes>,
    },
    Response {
        channel: ChannelId,
        id: Id,
        payload: Option<Bytes>,
    },
    RequestCancellation {
        io_id: IoId,
    },
    ResponseCancellation {
        channel: ChannelId,
        id: Id,
    },
    Error {
        channel: ChannelId,
        id: Id,
        payload: Bytes,
    },
}

impl QueuedItem {
    #[inline(always)]
    fn is_request(&self) -> bool {
        matches!(self, QueuedItem::Request { .. })
    }

    fn is_multi_frame(&self, max_frame_size: u32) -> bool {
        todo!()
    }

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

fn x(q: QueuedItem) {
    match q {
        QueuedItem::Request {
            io_id,
            channel,
            payload,
        } => todo!(),
        QueuedItem::Response {
            id,
            channel,
            payload,
        } => todo!(),
        QueuedItem::RequestCancellation { io_id } => todo!(),
        QueuedItem::ResponseCancellation { id, channel } => todo!(),
        QueuedItem::Error {
            id,
            channel,
            payload,
        } => todo!(),
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
    #[error("error sent to peer")]
    ErrorSent(OutgoingFrame),
    #[error("local protocol violation")]
    /// Local protocol violation - caller violated the crate's API.
    LocalProtocolViolation(#[from] LocalProtocolViolation),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IoId(u128);

pub struct IoCore<const N: usize, R, W> {
    /// The actual protocol state.
    juliet: JulietProtocol<N>,

    /// Underlying transport, reader.
    reader: R,
    /// Underlying transport, writer.
    writer: W,
    /// Read buffer for incoming data.
    buffer: BytesMut,
    /// How many more bytes are required until the next par
    bytes_until_next_parse: usize,

    /// The frame in the process of being sent, maybe be partially transferred.
    current_frame: Option<OutgoingFrame>,
    /// The header of the current multi-frame transfer.
    active_multi_frame: [Option<Header>; N],
    /// Frames that can be sent next.
    ready_queue: VecDeque<FrameIter>,
    /// Messages queued that are not yet ready to send.
    wait_queue: [VecDeque<QueuedItem>; N],
    /// Receiver for new items to send.
    receiver: Receiver<QueuedItem>,
    /// Mapping for outgoing requests, mapping internal IDs to public ones.
    request_map: HashMap<IoId, (ChannelId, RequestState)>,

    /// Shared data across handles and core.
    shared: Arc<IoShared<N>>,
}

#[derive(Copy, Clone, Debug)]
enum RequestState {
    /// The request is currently waiting and thus has not been assigned an ID yet.
    Waiting,
    /// The request has been sent.
    Allocated {
        /// ID assigned by the protocol core.
        id: Id,
    },
    /// The request has been sent out.
    Sent { id: Id },
    /// Request has been cancelled, we are waiting for the allocated ID to be reused.
    CancellationPending,
    /// Request has been sent, but a cancellation has been sent shortly after.
    CancellationSent { id: Id },
}

struct IoShared<const N: usize> {
    /// Number of requests already buffered per channel.
    requests_buffered: [AtomicUsize; N],
    /// Maximum allowed number of requests to buffer per channel.
    requests_limit: [usize; N],
}

impl<const N: usize> IoShared<N> {
    fn next_id(&self) -> IoId {
        todo!()
    }
}

#[derive(Debug)]
pub enum IoEvent {
    CompletedRead(CompletedRead),
    RemoteClosed,
    LocalShutdown,
}

impl IoEvent {
    #[inline(always)]
    fn should_shutdown(&self) -> bool {
        match self {
            IoEvent::CompletedRead(_) => false,
            IoEvent::RemoteClosed => true,
            IoEvent::LocalShutdown => true,
        }
    }
}

impl<const N: usize, R, W> IoCore<N, R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub async fn next_event(&mut self) -> Result<IoEvent, CoreError> {
        loop {
            if self.bytes_until_next_parse == 0 {
                match self.juliet.process_incoming(&mut self.buffer) {
                    Outcome::Incomplete(n) => {
                        // Simply reset how many bytes we need until the next parse.
                        self.bytes_until_next_parse = n.get() as usize;
                    }
                    Outcome::Fatal(err) => self.handle_fatal_read_err(err),
                    Outcome::Success(successful_read) => {
                        return self.handle_completed_read(successful_read);
                    }
                }
            }

            tokio::select! {
                biased;  // We do not need the bias, but we want to avoid randomness overhead.

                // Writing outgoing data:
                write_result = self.writer.write_all_buf(self.current_frame.as_mut().unwrap())
                    , if self.current_frame.is_some() => {
                    write_result.map_err(CoreError::WriteFailed)?;

                    let frame_sent = self.current_frame.take().unwrap();

                    if frame_sent.header().is_error() {
                        // We finished sending an error frame, time to exit.
                        return Err(CoreError::ErrorSent(frame_sent));
                    }

                    // Prepare the following frame, if any.
                    self.current_frame = self.ready_next_frame()?;
                }

                // Reading incoming data:
                read_result = read_atleast_bytesmut(&mut self.reader, &mut self.buffer, self.bytes_until_next_parse) => {
                    // Our read function will not return before `bytes_until_next_parse` has
                    // completed.
                    let bytes_read = read_result.map_err(CoreError::ReadFailed)?;

                    if bytes_read == 0 {
                        // Remote peer hung up.
                        return Ok(IoEvent::RemoteClosed);
                    }

                    self.bytes_until_next_parse = self.bytes_until_next_parse.saturating_sub(bytes_read);

                    // Fall through to start of loop, which parses data read.
                }

                mut incoming = self.receiver.recv() => {
                    let mut modified_channels = HashSet::new();

                    match incoming {
                        Some(item) => self.handle_incoming_item(item, &mut modified_channels)?;
                        None => {
                            return Ok(IoEvent::RemoteClosed);
                        }
                    }

                    todo!("loop over remainder")

                    if shutdown {
                        return Ok(IoEvent::LocalShutdown);
                    } else {
                        // Only process wait queue after having added all messages.
                        for channel in modified_channels {
                            self.process_wait_queue(channel)?;
                        }
                    }
                }
            }
        }
    }

    fn handle_completed_read(&mut self, read: CompletedRead) -> Result<IoEvent, CoreError> {
        match read {
            CompletedRead::ErrorReceived { header, data } => {
                // We've received an error, thus we should shut down the connection immediately.
                todo!()
            }
            CompletedRead::NewRequest { id, payload } => todo!(),
            CompletedRead::ReceivedResponse { id, payload } => todo!(),
            CompletedRead::RequestCancellation { id } => todo!(),
            CompletedRead::ResponseCancellation { id } => todo!(),
        }
    }

    fn handle_fatal_read_err(&mut self, err: OutgoingMessage) {
        todo!()
    }

    fn handle_incoming_item(
        &mut self,
        item: QueuedItem,
        channels_to_process: &mut HashSet<ChannelId>,
    ) -> Result<(), LocalProtocolViolation> {
        match item {
            QueuedItem::Request {
                io_id,
                channel,
                payload,
            } => {
                let active_multi_frame = self.active_multi_frame[channel.get() as usize];

                // Check if we can eagerly schedule, saving a trip through the wait queue.
                if item_is_ready(channel, &item, &self.juliet, &mut active_multi_frame) {
                    // The item is ready, we can directly schedule it and skip the wait queue.
                    let msg = self.juliet.create_request(channel, payload)?;
                    let id = msg.header().id();
                    self.ready_queue.push_back(msg.frames());
                    self.request_map
                        .insert(io_id, (channel, RequestState::Sent { id }));
                } else {
                    // Item not ready, put it into the wait queue.
                    self.wait_queue[channel.get() as usize].push_back(item);
                    self.request_map
                        .insert(io_id, (channel, RequestState::Waiting));
                    channels_to_process.insert(channel);
                }
            }
            QueuedItem::Response {
                id,
                channel,
                payload,
            } => {
                let active_multi_frame = self.active_multi_frame[channel.get() as usize];
                if item_is_ready(channel, &item, &self.juliet, &mut active_multi_frame) {
                    // The item is ready, we can directly schedule it and skip the wait queue.
                    if let Some(msg) = self.juliet.create_response(channel, id, payload)? {
                        self.ready_queue.push_back(msg.frames())
                    }
                } else {
                    // Item not ready, put it into the wait queue.
                    self.wait_queue[channel.get() as usize].push_back(item);
                    channels_to_process.insert(channel);
                }
            }
            QueuedItem::RequestCancellation { io_id } => {
                let (channel, state) = self.request_map.get(&io_id).expect("request map corrupted");
                match state {
                    RequestState::Waiting => {
                        // The request is in the wait or run queue, cancel it during processing.
                        self.request_map
                            .insert(io_id, (*channel, RequestState::CancellationPending));
                    }
                    RequestState::Allocated { id } => {
                        // Create the cancellation, but don't send it, since we caught it in time.
                        self.juliet.cancel_request(*channel, *id)?;
                        self.request_map
                            .insert(io_id, (*channel, RequestState::CancellationPending));
                    }
                    RequestState::Sent { id } => {
                        // Request has already been sent, schedule the cancellation message. We can
                        // bypass the wait queue, since cancellations are always valid to add. We'll
                        // also add it to the front of the queue to ensure they arrive in time.

                        if let Some(msg) = self.juliet.cancel_request(*channel, *id)? {
                            self.ready_queue.push_front(msg.frames());
                        }
                    }
                    RequestState::CancellationPending
                    | RequestState::CancellationSent { id: _ } => {
                        // Someone copied the `IoId`, we got a duplicated cancellation. Do nothing.
                    }
                }
            }
            QueuedItem::ResponseCancellation { id, channel } => {
                // `juliet` already tracks whether we still need to send the cancellation.
                // Unlike requests, we do not attempt to fish responses out of the queue,
                // cancelling a response after it has been created should be rare.
                if let Some(msg) = self.juliet.cancel_response(channel, id)? {
                    self.ready_queue.push_back(msg.frames());
                }
            }
            QueuedItem::Error {
                id,
                channel,
                payload,
            } => {
                // Errors go straight to the front of the line.
                let msg = self.juliet.custom_error(channel, id, payload)?;
                self.ready_queue.push_front(msg.frames());
            }
        }

        Ok(())
    }

    /// Clears a potentially finished frame and returns the best next frame to send.
    ///
    /// Returns `None` if no frames are ready to be sent. Note that there may be frames waiting
    /// that cannot be sent due them being multi-frame messages when there already is a multi-frame
    /// message in progress, or request limits being hit.
    fn ready_next_frame(&mut self) -> Result<Option<OutgoingFrame>, LocalProtocolViolation> {
        debug_assert!(self.current_frame.is_none()); // Must be guaranteed by caller.

        // Try to fetch a frame from the run queue. If there is nothing, we are stuck for now.
        let (frame, more) = match self.ready_queue.pop_front() {
            Some(item) => item,
            None => return Ok(None),
        }
        // Queue is empty, there is no next frame.
        .next_owned(self.juliet.max_frame_size());

        // If there are more frames after this one, schedule them again.
        if let Some(next_frame_iter) = more {
            self.ready_queue.push_back(next_frame_iter);
        } else {
            // No additional frames, check if we are about to finish a multi-frame transfer.
            let about_to_finish = frame.header();
            if let Some(ref active_multi) =
                self.active_multi_frame[about_to_finish.channel().get() as usize]
            {
                if about_to_finish == *active_multi {
                    // Once the scheduled frame is processed, we will finished the multi-frame
                    // transfer, so we can allow for the next multi-frame transfer to be scheduled.
                    self.active_multi_frame[about_to_finish.channel().get() as usize] = None;

                    // There is a chance another multi-frame messages became ready now.
                    self.process_wait_queue(about_to_finish.channel())?;
                }
            }
        }

        Ok(Some(frame))
    }

    /// Process the wait queue, moving messages that are ready to be sent to the ready queue.
    fn process_wait_queue(&mut self, channel: ChannelId) -> Result<(), LocalProtocolViolation> {
        // TODO: Rewrite, factoring out functions from `handle_incoming`.

        let active_multi_frame = &self.active_multi_frame[channel.get() as usize];
        let wait_queue = &mut self.wait_queue[channel.get() as usize];
        for _ in 0..(wait_queue.len()) {
            // Note: We do not use `drain` here, since we want to modify in-place. `retain` is also
            //       not used, since it does not allow taking out items by-value. An alternative
            //       might be sorting the list and splitting off the candidates instead.
            let item = wait_queue
                .pop_front()
                .expect("did not expect to run out of items");

            if item_is_ready(channel, &item, &self.juliet, active_multi_frame) {
                match item {
                    QueuedItem::Request { payload } => {
                        let msg = self.juliet.create_request(channel, payload)?;
                        self.ready_queue.push_back(msg.frames());
                    }
                    QueuedItem::Response { io_id: id, payload } => {
                        if let Some(msg) = self.juliet.create_response(channel, id, payload)? {
                            self.ready_queue.push_back(msg.frames());
                        }
                    }
                    QueuedItem::RequestCancellation { io_id: id } => {
                        if let Some(msg) = self.juliet.cancel_request(channel, id)? {
                            self.ready_queue.push_back(msg.frames());
                        }
                    }
                    QueuedItem::ResponseCancellation { io_id: id } => {
                        if let Some(msg) = self.juliet.cancel_response(channel, id)? {
                            self.ready_queue.push_back(msg.frames());
                        }
                    }
                    QueuedItem::Error { id, payload } => {
                        let msg = self.juliet.custom_error(channel, id, payload)?;
                        // Errors go into the front.
                        self.ready_queue.push_front(msg.frames());
                    }
                }
            } else {
                wait_queue.push_back(item);
            }
        }

        Ok(())
    }

    fn into_stream(self) -> impl Stream<Item = Result<IoEvent, CoreError>> {
        futures::stream::unfold(Some(self), |state| async {
            let mut this = state?;
            let rv = this.next_event().await;

            // Check if this was the last event. We shut down on close or any error.
            if rv.as_ref().map(IoEvent::should_shutdown).unwrap_or(true) {
                Some((rv, None))
            } else {
                Some((rv, Some(this)))
            }
        })
    }
}

fn item_is_ready<const N: usize>(
    channel: ChannelId,
    item: &QueuedItem,
    juliet: &JulietProtocol<N>,
    active_multi_frame: &Option<Header>,
) -> bool {
    // Check if we cannot schedule due to the message exceeding the request limit.
    if item.is_request() {
        if !juliet
            .allowed_to_send_request(channel)
            .expect("should not be called with invalid channel")
        {
            return false;
        }
    }

    // Check if we cannot schedule due to the message being multi-frame and there being a
    // multi-frame send in progress:
    if active_multi_frame.is_some() {
        if item.is_multi_frame(juliet.max_frame_size()) {
            return false;
        }
    }

    // Otherwise, this should be a legitimate add to the run queue.
    true
}

struct IoHandle<const N: usize> {
    shared: Arc<IoShared<N>>,
    /// Sender for queue items.
    sender: Sender<QueuedItem>,
    next_io_id: u128,
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
        &self,
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
                // We successfully increment the count.
                let io_id = IoId(self.next_io_id);

                // Does not roll over before at least 10^18 zettabytes have been sent.
                self.next_io_id = self.next_io_id.wrapping_add(1);

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
/// `target` bytes have been read.
///
/// Will automatically retry if an [`io::ErrorKind::Interrupted`] is returned.
///
/// # Cancellation safety
///
/// This function is cancellation safe in the same way that [`AsyncReadExt::read_buf`] is.
async fn read_atleast_bytesmut<'a, R>(
    reader: &'a mut R,
    buf: &mut BytesMut,
    target: usize,
) -> io::Result<usize>
where
    R: AsyncReadExt + Sized + Unpin,
{
    let mut bytes_read = 0;
    buf.reserve(target);

    while bytes_read < target {
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
