//! RPC layer.
//!
//! The outermost layer of the `juliet` stack, combines the underlying [`io`](crate::io) and
//! [`protocol`](crate::protocol) layers into a convenient RPC system.
//!
//! The term RPC is used somewhat inaccurately here, as the crate does _not_ deal with the actual
//! method calls or serializing arguments, but only provides the underlying request/response system.
//!
//! ## Usage
//!
//! The RPC system is configured by setting up an [`RpcBuilder<N>`], which in turn requires an
//! [`IoCoreBuilder<N>`] and [`ProtocolBuilder<N>`](crate::protocol::ProtocolBuilder) (see the
//! [`io`](crate::io) and [`protocol`](crate::protocol) module documentation for details), with `N`
//! denoting the number of preconfigured channels.
//!
//! Once a connection has been established, [`RpcBuilder::build`] is used to construct a
//! [`JulietRpcClient`] and [`JulietRpcServer`] pair, the former being used use to make remote
//! procedure calls, while latter is used to answer them. Note that
//! [`JulietRpcServer::next_request`] must continuously be called regardless of whether requests are
//! handled locally, since the function is also responsible for performing the underlying IO.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    fmt::{self, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;

use once_cell::sync::OnceCell;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Notify,
    },
    time::Instant,
};

use crate::{
    io::{
        CoreError, EnqueueError, Handle, IoCore, IoCoreBuilder, IoEvent, IoId, RequestHandle,
        RequestTicket, ReservationError,
    },
    protocol::LocalProtocolViolation,
    util::PayloadFormat,
    ChannelId, Id,
};

/// Builder for a new RPC interface.
pub struct RpcBuilder<const N: usize> {
    /// The IO core builder used.
    core: IoCoreBuilder<N>,
    /// Whether or not to enable timeout bubbling.
    bubble_timeouts: bool,
    /// The default timeout for created requests.
    default_timeout: Option<Duration>,
}

impl<const N: usize> RpcBuilder<N> {
    /// Constructs a new RPC builder.
    ///
    /// The builder can be reused to create instances for multiple connections.
    pub fn new(core: IoCoreBuilder<N>) -> Self {
        RpcBuilder {
            core,
            bubble_timeouts: false,
            default_timeout: None,
        }
    }

    /// Enable timeout bubbling.
    ///
    /// If enabled, any timeout from an RPC call will also cause an error in
    /// [`JulietRpcServer::next_request`], specifically an [`RpcServerError::FatalTimeout`], which
    /// will cause a severing of the connection.
    ///
    /// This feature can be used to implement a liveness check, causing any timed out request to be
    /// considered fatal. Note that under high load a remote server may take time to answer, thus it
    /// is best not to set too aggressive timeout values on requests if this setting is enabled.
    pub fn bubble_timeouts(mut self, bubble_timeouts: bool) -> Self {
        self.bubble_timeouts = bubble_timeouts;
        self
    }

    /// Sets a default timeout.
    ///
    /// If set, a default timeout will be applied to every request made through the created
    /// [`JulietRpcClient`].
    pub fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.default_timeout = Some(default_timeout);
        self
    }

    /// Creates new RPC client and server instances.
    pub fn build<R, W>(
        &self,
        reader: R,
        writer: W,
    ) -> (JulietRpcClient<N>, JulietRpcServer<N, R, W>) {
        let (core, core_handle) = self.core.build(reader, writer);

        let (new_request_sender, new_requests_receiver) = mpsc::unbounded_channel();

        let client = JulietRpcClient {
            new_request_sender,
            request_handle: core_handle.clone(),
            default_timeout: self.default_timeout,
        };
        let server = JulietRpcServer {
            core,
            handle: core_handle.downgrade(),
            pending: Default::default(),
            new_requests_receiver,
            timeouts: BinaryHeap::new(),
            bubble_timeouts: self.bubble_timeouts,
        };

        (client, server)
    }
}

/// Juliet RPC client.
///
/// The client is used to create new RPC calls through [`JulietRpcClient::create_request`].
#[derive(Clone, Debug)]
pub struct JulietRpcClient<const N: usize> {
    /// Sender for requests to be send through.
    new_request_sender: UnboundedSender<NewOutgoingRequest>,
    /// Handle to IO core.
    request_handle: RequestHandle<N>,
    /// Default timeout for requests.
    default_timeout: Option<Duration>,
}

/// Builder for an outgoing RPC request.
///
/// Once configured, it can be sent using either
/// [`queue_for_sending`](JulietRpcRequestBuilder::queue_for_sending) or
/// [`try_queue_for_sending`](JulietRpcRequestBuilder::try_queue_for_sending), returning a
/// [`RequestGuard`], which can be used to await the results of the request.
#[derive(Debug)]
pub struct JulietRpcRequestBuilder<'a, const N: usize> {
    client: &'a JulietRpcClient<N>,
    channel: ChannelId,
    payload: Option<Bytes>,
    timeout: Option<Duration>,
}

/// Juliet RPC Server.
///
/// The server's purpose is to produce incoming RPC calls and run the underlying IO layer. For this
/// reason it is important to repeatedly call [`next_request`](Self::next_request), see the method
/// documentation for details.
///
/// ## Shutdown
///
/// The server will automatically be shutdown if the last [`JulietRpcClient`] is dropped.
#[derive(Debug)]
pub struct JulietRpcServer<const N: usize, R, W> {
    /// The `io` module core used by this server.
    core: IoCore<N, R, W>,
    /// Handle to the `IoCore`, cloned for clients.
    handle: Handle,
    /// Map of requests that are still pending.
    pending: HashMap<IoId, Arc<RequestGuardInner>>,
    /// Receiver for request scheduled by `JulietRpcClient`s.
    new_requests_receiver: UnboundedReceiver<NewOutgoingRequest>,
    /// Heap of pending timeouts.
    timeouts: BinaryHeap<Reverse<(Instant, IoId)>>,
    /// Whether or not to bubble up timed out requests, making them an [`RpcServerError`].
    bubble_timeouts: bool,
}

/// Internal structure representing a new outgoing request.
#[derive(Debug)]
struct NewOutgoingRequest {
    /// The already reserved ticket.
    ticket: RequestTicket,
    /// Request guard to store results.
    guard: Arc<RequestGuardInner>,
    /// Payload of the request.
    payload: Option<Bytes>,
    /// When the request is supposed to time out.
    expires: Option<Instant>,
}

impl Display for NewOutgoingRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "NewOutgoingRequest {{ ticket: {}", self.ticket,)?;
        if let Some(ref expires) = self.expires {
            write!(f, ", expires: {:?}", expires)?;
        }
        if let Some(ref payload) = self.payload {
            write!(f, ", payload: {}", PayloadFormat(payload))?;
        }
        f.write_str(" }}")
    }
}

#[derive(Debug)]
struct RequestGuardInner {
    /// The returned response of the request.
    outcome: OnceCell<Result<Option<Bytes>, RequestError>>,
    /// A notifier for when the result arrives.
    ready: Option<Notify>,
}

impl RequestGuardInner {
    fn new() -> Self {
        RequestGuardInner {
            outcome: OnceCell::new(),
            ready: Some(Notify::new()),
        }
    }

    fn set_and_notify(&self, value: Result<Option<Bytes>, RequestError>) {
        if self.outcome.set(value).is_ok() {
            // If this is the first time the outcome is changed, notify exactly once.
            if let Some(ref ready) = self.ready {
                ready.notify_one()
            }
        };
    }
}

impl<const N: usize> JulietRpcClient<N> {
    /// Creates a new RPC request builder.
    ///
    /// The returned builder can be used to create a single request on the given channel.
    pub fn create_request(&self, channel: ChannelId) -> JulietRpcRequestBuilder<N> {
        JulietRpcRequestBuilder {
            client: self,
            channel,
            payload: None,
            timeout: self.default_timeout,
        }
    }
}

/// An error produced by the RPC error.
#[derive(Debug, Error)]
pub enum RpcServerError {
    /// An [`IoCore`] error.
    #[error(transparent)]
    CoreError(#[from] CoreError),
    /// At least `count` requests timed out, and the RPC layer is configured to bubble up timeouts.
    #[error("connection error after {count} request(s) timed out")]
    FatalTimeout {
        /// Number of requests that timed out at once.
        count: usize,
    },
}

impl<const N: usize, R, W> JulietRpcServer<N, R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Produce the next request from the peer.
    ///
    /// Runs the underlying IO until another [`IncomingRequest`] has been produced by the remote
    /// peer. On success, this function should be called again immediately.
    ///
    /// On a regular shutdown (`None` returned) or an error ([`RpcServerError`] returned), a caller
    /// must stop calling [`next_request`](Self::next_request) and should drop the entire
    /// [`JulietRpcServer`].
    ///
    /// **Important**: Even if the local peer is not intending to handle any requests, this function
    /// must still be called, since it drives the underlying IO system. It is also highly recommend
    /// to offload the actual handling of requests to a separate task and return to calling
    /// `next_request` as soon as possible.
    pub async fn next_request(&mut self) -> Result<Option<IncomingRequest>, RpcServerError> {
        loop {
            let now = Instant::now();

            // Process all the timeouts.
            let (deadline, timed_out) = self.process_timeouts(now);

            if self.bubble_timeouts && timed_out > 0 {
                return Err(RpcServerError::FatalTimeout { count: timed_out });
            };
            let timeout_check = tokio::time::sleep_until(deadline);

            tokio::select! {
                biased;

                _ = timeout_check => {
                    // Enough time has elapsed that we need to check for timeouts, which we will
                    // do the next time we loop.
                    #[cfg(feature = "tracing")]
                    tracing::trace!("timeout check");
                }

                opt_new_request = self.new_requests_receiver.recv() => {
                    #[cfg(feature = "tracing")]
                    {
                        if let Some(ref new_request) = opt_new_request {
                            tracing::debug!(%new_request, "trying to enqueue");
                        }
                    }
                    if let Some(NewOutgoingRequest { ticket, guard, payload, expires }) = opt_new_request {
                        match self.handle.enqueue_request(ticket, payload) {
                            Ok(io_id) => {
                                // The request will be sent out, store it in our pending map.
                                self.pending.insert(io_id, guard);

                                // If a timeout has been configured, add it to the timeouts map.
                                if let Some(expires) = expires {
                                    self.timeouts.push(Reverse((expires, io_id)));
                                }
                            },
                            Err(payload) => {
                                // Failed to send -- time to shut down.
                                guard.set_and_notify(Err(RequestError::RemoteClosed(payload)))
                            }
                        }
                    } else {
                        // The client has been dropped, time for us to shut down as well.
                        #[cfg(feature = "tracing")]
                        tracing::info!("last client dropped locally, shutting down");

                        return Ok(None);
                    }
                }

                event_result = self.core.next_event() => {
                    #[cfg(feature = "tracing")]
                    {
                        match event_result {
                            Err(ref err) => {
                                if matches!(err, CoreError::LocalProtocolViolation(_)) {
                                    tracing::warn!(%err, "error");
                                } else {
                                    tracing::info!(%err, "error");
                                }
                            }
                            Ok(None) => {
                                tracing::info!("received remote close");
                            }
                            Ok(Some(ref event)) => {
                                tracing::debug!(%event, "received");
                            }
                        }
                    }
                    if let Some(event) = event_result? {
                        match event {
                            IoEvent::NewRequest {
                                channel,
                                id,
                                payload,
                            } => return Ok(Some(IncomingRequest {
                                channel,
                                id,
                                payload,
                                handle: Some(self.handle.clone()),
                            })),
                            IoEvent::RequestCancelled { .. } => {
                                // Request cancellation is currently not implemented; there is no
                                // harm in sending the reply.
                            },
                            IoEvent::ReceivedResponse { io_id, payload } => {
                                match self.pending.remove(&io_id) {
                                    None => {
                                        // The request has been cancelled on our end, no big deal.
                                    }
                                    Some(guard) => {
                                        guard.set_and_notify(Ok(payload))
                                    }
                                }
                            },
                            IoEvent::ReceivedCancellationResponse { io_id } => {
                                match self.pending.remove(&io_id) {
                                    None => {
                                        // The request has been cancelled on our end, no big deal.
                                    }
                                    Some(guard) => {
                                        guard.set_and_notify(Err(RequestError::RemoteCancelled))
                                    }
                                }
                            },
                        }
                    } else {
                        return Ok(None)
                    }
                }
            };
        }
    }

    /// Process all pending timeouts, setting and notifying `RequestError::TimedOut` on timeout.
    ///
    /// Returns the duration until the next timeout check needs to take place if timeouts are not
    /// modified in the interim, and the number of actual timeouts.
    fn process_timeouts(&mut self, now: Instant) -> (Instant, usize) {
        let is_expired = |t: &Reverse<(Instant, IoId)>| t.0 .0 <= now;

        // Track the number of actual timeouts hit.
        let mut timed_out = 0;

        for item in drain_heap_while(&mut self.timeouts, is_expired) {
            let (_, io_id) = item.0;

            // If not removed already through other means, set and notify about timeout.
            if let Some(guard_ref) = self.pending.remove(&io_id) {
                #[cfg(feature = "tracing")]
                tracing::debug!(%io_id, "timeout due to response not received in time");
                guard_ref.set_and_notify(Err(RequestError::TimedOut));

                // We also need to send a cancellation.
                if self.handle.enqueue_request_cancellation(io_id).is_err() {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(%io_id, "dropping timeout cancellation, remote already closed");
                }

                // Increase timed out count.
                timed_out += 1;
            }
        }
        // Calculate new delay for timeouts.
        let deadline = if let Some(Reverse((when, _))) = self.timeouts.peek() {
            *when
        } else {
            // 1 hour dummy sleep, since we cannot have a conditional future.
            now + Duration::from_secs(3600)
        };

        (deadline, timed_out)
    }
}

impl<const N: usize, R, W> Drop for JulietRpcServer<N, R, W> {
    fn drop(&mut self) {
        // When the server is dropped, ensure all waiting requests are informed.
        self.new_requests_receiver.close();

        for (_io_id, guard) in self.pending.drain() {
            guard.set_and_notify(Err(RequestError::Shutdown));
        }

        while let Ok(NewOutgoingRequest {
            ticket: _,
            guard,
            payload,
            expires: _,
        }) = self.new_requests_receiver.try_recv()
        {
            guard.set_and_notify(Err(RequestError::RemoteClosed(payload)))
        }
    }
}

impl<'a, const N: usize> JulietRpcRequestBuilder<'a, N> {
    /// Recovers a payload from the request builder.
    pub fn into_payload(self) -> Option<Bytes> {
        self.payload
    }

    /// Sets the payload for the request.
    ///
    /// By default, no payload is included.
    pub fn with_payload(mut self, payload: Bytes) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets the timeout for the request.
    ///
    /// By default, there is an infinite timeout.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Schedules a new request on an outgoing channel.
    ///
    /// If there is no buffer space available for the request, blocks until there is.
    pub async fn queue_for_sending(self) -> RequestGuard {
        let ticket = match self
            .client
            .request_handle
            .reserve_request(self.channel)
            .await
        {
            Some(ticket) => ticket,
            None => {
                // We cannot queue the request, since the connection was closed.
                return RequestGuard::new_error(RequestError::RemoteClosed(self.payload));
            }
        };

        self.do_enqueue_request(ticket)
    }

    /// Schedules a new request on an outgoing channel if space is available.
    ///
    /// If no space is available, returns the [`JulietRpcRequestBuilder`] as an `Err` value, so it
    /// can be retried later.
    pub fn try_queue_for_sending(self) -> Result<RequestGuard, Self> {
        let ticket = match self.client.request_handle.try_reserve_request(self.channel) {
            Ok(ticket) => ticket,
            Err(ReservationError::Closed) => {
                return Ok(RequestGuard::new_error(RequestError::RemoteClosed(
                    self.payload,
                )));
            }
            Err(ReservationError::NoBufferSpaceAvailable) => {
                return Err(self);
            }
        };

        Ok(self.do_enqueue_request(ticket))
    }

    #[inline(always)]
    fn do_enqueue_request(self, ticket: RequestTicket) -> RequestGuard {
        let inner = Arc::new(RequestGuardInner::new());

        // If a timeout is set, calculate expiration time.
        let expires = if let Some(timeout) = self.timeout {
            match Instant::now().checked_add(timeout) {
                Some(expires) => Some(expires),
                None => {
                    // The timeout is so high that the resulting `Instant` would overflow.
                    return RequestGuard::new_error(RequestError::TimeoutOverflow(timeout));
                }
            }
        } else {
            None
        };

        match self.client.new_request_sender.send(NewOutgoingRequest {
            ticket,
            guard: inner.clone(),
            payload: self.payload,
            expires,
        }) {
            Ok(()) => RequestGuard { inner },
            Err(send_err) => {
                RequestGuard::new_error(RequestError::RemoteClosed(send_err.0.payload))
            }
        }
    }
}

/// An RPC request error.
///
/// Describes the reason a request did not yield a response.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RequestError {
    /// Remote closed, could not send.
    ///
    /// The request was never sent out, since the underlying [`IoCore`] was already shut down when
    /// it was made.
    #[error("remote closed connection before request could be sent")]
    RemoteClosed(Option<Bytes>),
    /// Sent, but never received a reply.
    ///
    /// Request was sent, but we never received anything back before the [`IoCore`] was shut down.
    #[error("never received reply before remote closed connection")]
    Shutdown,
    /// Local timeout.
    ///
    /// The request was cancelled on our end due to a timeout.
    #[error("request timed out")]
    TimedOut,
    /// Local timeout overflow.
    ///
    /// The given timeout would cause a clock overflow.
    #[error("requested timeout ({0:?}) would cause clock overflow")]
    TimeoutOverflow(Duration),
    /// Remote responded with cancellation.
    ///
    /// Instead of sending a response, the remote sent a cancellation.
    #[error("remote cancelled our request")]
    RemoteCancelled,
    /// Cancelled locally.
    ///
    /// Request was cancelled on our end.
    #[error("request cancelled locally")]
    Cancelled,
    /// API misuse.
    ///
    /// Either the API was misused, or a bug in this crate appeared.
    #[error("API misused or other internal error")]
    Error(LocalProtocolViolation),
}

/// Handle to an in-flight outgoing request.
///
/// The existence of a [`RequestGuard`] indicates that a request has been made or is ongoing. It
/// can also be used to attempt to [`cancel`](RequestGuard::cancel) the request, or retrieve its
/// values using [`wait_for_response`](RequestGuard::wait_for_response) or
/// [`try_get_response`](RequestGuard::try_get_response).
#[derive(Debug)]
#[must_use = "dropping the request guard will immediately cancel the request"]
pub struct RequestGuard {
    /// Shared reference to outcome data.
    inner: Arc<RequestGuardInner>,
}

impl RequestGuard {
    /// Creates a new request guard with no shared data that is already resolved to an error.
    fn new_error(error: RequestError) -> Self {
        let outcome = OnceCell::new();
        outcome
            .set(Err(error))
            .expect("newly constructed cell should always be empty");
        RequestGuard {
            inner: Arc::new(RequestGuardInner {
                outcome,
                ready: None,
            }),
        }
    }

    /// Cancels the request.
    ///
    /// May cause the request to not be sent if it is still in the queue, or a cancellation to be
    /// sent if it already left the local machine.
    pub fn cancel(mut self) {
        self.do_cancel();

        self.forget()
    }

    fn do_cancel(&mut self) {
        // TODO: Implement eager cancellation locally, potentially removing this request from the
        //       outbound queue.
        // TODO: Implement actual sending of the cancellation.
    }

    /// Forgets the request was made.
    ///
    /// Similar to [`cancel`](Self::cancel), except that it will not cause an actual cancellation,
    /// so the peer will likely perform all the work. The response will be discarded.
    pub fn forget(self) {
        // Just do nothing.
    }

    /// Waits for a response to come back.
    ///
    /// Blocks until a response, cancellation or error has been received for this particular
    /// request.
    ///
    /// If a response has been received, the optional [`Bytes`] of the payload will be returned.
    ///
    /// On an error, including a cancellation by the remote, returns a [`RequestError`].
    pub async fn wait_for_response(self) -> Result<Option<Bytes>, RequestError> {
        // Wait for notification.
        if let Some(ref ready) = self.inner.ready {
            ready.notified().await;
        }

        self.take_inner()
    }

    /// Waits for the response, non-blockingly.
    ///
    /// Like [`wait_for_response`](Self::wait_for_response), except that instead of waiting, it will
    /// return `Err(self)` if the peer was not ready yet.
    pub fn try_get_response(self) -> Result<Result<Option<Bytes>, RequestError>, Self> {
        if self.inner.outcome.get().is_some() {
            Ok(self.take_inner())
        } else {
            Err(self)
        }
    }

    fn take_inner(self) -> Result<Option<Bytes>, RequestError> {
        // TODO: Best to move `Notified` + `OnceCell` into a separate struct for testing and
        // upholding these invariants, avoiding the extra clones.

        self.inner
            .outcome
            .get()
            .expect("should not have called notified without setting cell contents")
            .clone()
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.do_cancel();
    }
}

/// An incoming request from a peer.
///
/// Every request should be answered using either the [`IncomingRequest::cancel()`] or
/// [`IncomingRequest::respond()`] methods.
///
/// ## Automatic cleanup
///
/// If dropped, [`IncomingRequest::cancel()`] is called automatically, which will cause a
/// cancellation to be sent.
#[derive(Debug)]
#[must_use]
pub struct IncomingRequest {
    /// Channel the request was sent on.
    channel: ChannelId,
    /// Id chosen by peer for the request.
    id: Id,
    /// Payload attached to request.
    payload: Option<Bytes>,
    /// Handle to [`IoCore`] to send a reply.
    handle: Option<Handle>,
}

impl Display for IncomingRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IncomingRequest {{ channel: {}, id: {}, payload: ",
            self.channel, self.id
        )?;

        if let Some(ref payload) = self.payload {
            write!(f, "{} bytes }}", payload.len())
        } else {
            f.write_str("none>")
        }
    }
}

impl IncomingRequest {
    /// Returns the [`ChannelId`] of the channel the request arrived on.
    #[inline(always)]
    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    /// Returns the [`Id`] of the request.
    #[inline(always)]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Returns a reference to the payload, if any.
    #[inline(always)]
    pub const fn payload(&self) -> &Option<Bytes> {
        &self.payload
    }

    /// Returns a mutable reference to the payload, if any.
    ///
    /// Typically used in conjunction with [`Option::take()`].
    #[inline(always)]
    pub fn payload_mut(&mut self) -> &mut Option<Bytes> {
        &mut self.payload
    }

    /// Enqueue a response to be sent out.
    ///
    /// The response will contain the specified `payload`, sent on a best effort basis. Responses
    /// will never be rejected on a basis of memory.
    #[inline]
    pub fn respond(mut self, payload: Option<Bytes>) {
        if let Some(handle) = self.handle.take() {
            if let Err(err) = handle.enqueue_response(self.channel, self.id, payload) {
                match err {
                    EnqueueError::Closed(_) => {
                        // Do nothing, just discard the response.
                    }
                    EnqueueError::BufferLimitHit(_) => {
                        // TODO: Add seperate type to avoid this.
                        unreachable!("cannot hit request limit when responding")
                    }
                }
            }
        }
    }

    /// Cancel the request.
    ///
    /// This will cause a cancellation to be sent back.
    #[inline(always)]
    pub fn cancel(mut self) {
        self.do_cancel();
    }

    fn do_cancel(&mut self) {
        if let Some(handle) = self.handle.take() {
            if let Err(err) = handle.enqueue_response_cancellation(self.channel, self.id) {
                match err {
                    EnqueueError::Closed(_) => {
                        // Do nothing, just discard the response.
                    }
                    EnqueueError::BufferLimitHit(_) => {
                        unreachable!("cannot hit request limit when responding")
                    }
                }
            }
        }
    }
}

impl Drop for IncomingRequest {
    #[inline(always)]
    fn drop(&mut self) {
        self.do_cancel();
    }
}

/// An iterator draining items out of a heap based on a predicate.
///
/// See [`drain_heap_while`] for details.
struct DrainConditional<'a, T, F> {
    /// Heap to be drained.
    heap: &'a mut BinaryHeap<T>,
    /// Predicate function to determine whether or not to drain a specific element.
    predicate: F,
}

/// Removes items from the top of a heap while a given predicate is true.
fn drain_heap_while<T, F: FnMut(&T) -> bool>(
    heap: &mut BinaryHeap<T>,
    predicate: F,
) -> DrainConditional<'_, T, F> {
    DrainConditional { heap, predicate }
}

impl<'a, T, F> Iterator for DrainConditional<'a, T, F>
where
    F: FnMut(&T) -> bool,
    T: Ord + PartialOrd + 'static,
{
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let candidate = self.heap.peek()?;
        if (self.predicate)(candidate) {
            Some(
                self.heap
                    .pop()
                    .expect("did not expect heap top to disappear"),
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BinaryHeap, sync::Arc, time::Duration};

    use bytes::Bytes;
    use futures::FutureExt;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};
    use tracing::{span, Instrument, Level};

    use crate::{
        io::IoCoreBuilder,
        protocol::ProtocolBuilder,
        rpc::{RequestError, RpcBuilder},
        ChannelConfiguration, ChannelId,
    };

    use super::{
        drain_heap_while, JulietRpcClient, JulietRpcServer, RequestGuard, RequestGuardInner,
    };

    #[allow(clippy::type_complexity)] // We'll allow it in testing.
    fn setup_peers<const N: usize>(
        builder: RpcBuilder<N>,
    ) -> (
        (
            JulietRpcClient<N>,
            JulietRpcServer<N, ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
        ),
        (
            JulietRpcClient<N>,
            JulietRpcServer<N, ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
        ),
    ) {
        let (peer_a_pipe, peer_b_pipe) = tokio::io::duplex(64);
        let peer_a = {
            let (reader, writer) = tokio::io::split(peer_a_pipe);
            builder.build(reader, writer)
        };
        let peer_b = {
            let (reader, writer) = tokio::io::split(peer_b_pipe);
            builder.build(reader, writer)
        };
        (peer_a, peer_b)
    }

    // It takes about 12 ms one-way for sound from the base of the Matterhorn to reach the summit,
    // so we expect a single yodel to echo within ~ 24 ms, which is use as a reference here.
    const ECHO_DELAY: Duration = Duration::from_millis(2 * 12);

    /// Runs an echo server in the background.
    ///
    /// The server keeps running as long as the future is polled.
    async fn run_echo_server<const N: usize>(
        server: (
            JulietRpcClient<N>,
            JulietRpcServer<N, ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
        ),
    ) {
        let (rpc_client, mut rpc_server) = server;

        while let Some(req) = rpc_server
            .next_request()
            .await
            .expect("error receiving request")
        {
            let payload = req.payload().clone();

            tokio::time::sleep(ECHO_DELAY).await;
            req.respond(payload);
        }

        drop(rpc_client);
    }

    /// Runs the necessary server functionality for the RPC client.
    async fn run_echo_client<const N: usize>(
        mut rpc_server: JulietRpcServer<N, ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
    ) {
        while let Some(inc) = rpc_server
            .next_request()
            .await
            .expect("client rpc_server error")
        {
            panic!("did not expect to receive {:?} on client", inc);
        }
    }

    /// Creates a channel configuration with test defaults.
    fn create_config() -> ChannelConfiguration {
        ChannelConfiguration::new()
            .with_max_request_payload_size(1024)
            .with_max_response_payload_size(1024)
            .with_request_limit(1)
    }

    /// Completely sets up an environment with a running echo server, returning a client.
    fn create_rpc_echo_server_env(channel_config: ChannelConfiguration) -> JulietRpcClient<2> {
        // Setup logging if not already set up.
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init()
            .ok(); // If setting up logging fails, another testing thread already initialized it.

        let builder = RpcBuilder::new(IoCoreBuilder::new(
            ProtocolBuilder::<2>::with_default_channel_config(channel_config),
        ));

        let (client, server) = setup_peers(builder);

        // Spawn the server.
        tokio::spawn(run_echo_server(server).instrument(span!(Level::ERROR, "server")));

        let (rpc_client, rpc_server) = client;

        // Run the background process for the client.
        tokio::spawn(run_echo_client(rpc_server).instrument(span!(Level::ERROR, "client")));

        rpc_client
    }

    #[tokio::test]
    async fn basic_smoke_test() {
        let rpc_client = create_rpc_echo_server_env(create_config());

        let payload = Bytes::from(&b"foobar"[..]);

        let response = rpc_client
            .create_request(ChannelId::new(0))
            .with_payload(payload.clone())
            .queue_for_sending()
            .await
            .wait_for_response()
            .await
            .expect("request failed");

        assert_eq!(response, Some(payload.clone()));

        // Create a second request with a timeout.
        let response_err = rpc_client
            .create_request(ChannelId::new(0))
            .with_payload(payload.clone())
            .with_timeout(ECHO_DELAY / 2)
            .queue_for_sending()
            .await
            .wait_for_response()
            .await;
        assert_eq!(response_err, Err(crate::rpc::RequestError::TimedOut));
    }

    #[tokio::test]
    async fn timeout_processed_in_correct_order() {
        // It's important to set a request limit higher than 1, so that both requests can be sent at
        // the same time.
        let rpc_client = create_rpc_echo_server_env(create_config().with_request_limit(3));

        let payload_short = Bytes::from(&b"timeout check short"[..]);
        let payload_long = Bytes::from(&b"timeout check long"[..]);

        // Sending two requests with different timeouts will result in both being added to the heap
        // of timeouts to check. If the internal heap is in the wrong order, the bigger timeout will
        // prevent the smaller one from being processed.

        let req_short = rpc_client
            .create_request(ChannelId::new(0))
            .with_payload(payload_short)
            .with_timeout(ECHO_DELAY / 2)
            .queue_for_sending()
            .await;

        let req_long = rpc_client
            .create_request(ChannelId::new(0))
            .with_payload(payload_long.clone())
            .with_timeout(ECHO_DELAY * 100)
            .queue_for_sending()
            .await;

        let result_short = req_short.wait_for_response().await;
        let result_long = req_long.wait_for_response().await;

        assert_eq!(result_short, Err(RequestError::TimedOut));
        assert_eq!(result_long, Ok(Some(payload_long)));

        // TODO: Ensure cancellation was sent. Right now, we can verify this in the logs, but it
        //       would be nice to have a test tailored to ensure this.
    }

    // TODO: Tests for timeout bubbling and default timeouts.

    #[test]
    fn request_guard_polls_waiting_with_no_response() {
        let inner = Arc::new(RequestGuardInner::new());
        let guard = RequestGuard { inner };

        // Initially, the guard should not have a response.
        let guard = guard
            .try_get_response()
            .expect_err("should not have a result");

        // Polling it should also result in a wait.
        let waiting = guard.wait_for_response();

        assert!(waiting.now_or_never().is_none());
    }

    #[test]
    fn request_guard_polled_early_returns_response_when_available() {
        let inner = Arc::new(RequestGuardInner::new());
        let guard = RequestGuard {
            inner: inner.clone(),
        };

        // Waiter created before response sent.
        let waiting = guard.wait_for_response();
        inner.set_and_notify(Ok(None));

        assert_eq!(waiting.now_or_never().expect("should poll ready"), Ok(None));
    }

    #[test]
    fn request_guard_polled_late_returns_response_when_available() {
        let inner = Arc::new(RequestGuardInner::new());
        let guard = RequestGuard {
            inner: inner.clone(),
        };

        inner.set_and_notify(Ok(None));

        // Waiter created after response sent.
        let waiting = guard.wait_for_response();

        assert_eq!(waiting.now_or_never().expect("should poll ready"), Ok(None));
    }

    #[test]
    fn request_guard_get_returns_correct_value_when_available() {
        let inner = Arc::new(RequestGuardInner::new());
        let guard = RequestGuard {
            inner: inner.clone(),
        };

        // Waiter created and polled before notification.
        let guard = guard
            .try_get_response()
            .expect_err("should not have a result");

        let payload_str = b"hello, world";
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str))));

        assert_eq!(
            guard.try_get_response().expect("should be ready"),
            Ok(Some(Bytes::from_static(payload_str)))
        );
    }

    #[test]
    fn request_guard_harmless_to_set_multiple_times() {
        // We want first write wins semantics here.
        let inner = Arc::new(RequestGuardInner::new());
        let guard = RequestGuard {
            inner: inner.clone(),
        };

        let payload_str = b"hello, world";
        let payload_str2 = b"goodbye, world";

        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));

        assert_eq!(
            guard.try_get_response().expect("should be ready"),
            Ok(Some(Bytes::from_static(payload_str)))
        );

        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
        inner.set_and_notify(Ok(Some(Bytes::from_static(payload_str2))));
    }

    #[test]
    fn drain_works() {
        let mut heap = BinaryHeap::new();

        heap.push(5);
        heap.push(3);
        heap.push(2);
        heap.push(7);
        heap.push(11);
        heap.push(13);

        assert!(drain_heap_while(&mut heap, |_| false).next().is_none());
        assert!(drain_heap_while(&mut heap, |&v| v > 14).next().is_none());

        assert_eq!(
            drain_heap_while(&mut heap, |&v| v > 10).collect::<Vec<_>>(),
            vec![13, 11]
        );

        assert_eq!(
            drain_heap_while(&mut heap, |&v| v > 10).collect::<Vec<_>>(),
            Vec::<i32>::new()
        );

        assert_eq!(
            drain_heap_while(&mut heap, |&v| v > 2).collect::<Vec<_>>(),
            vec![7, 5, 3]
        );

        assert_eq!(
            drain_heap_while(&mut heap, |_| true).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn drain_on_empty_works() {
        let mut empty_heap = BinaryHeap::<u32>::new();

        assert!(drain_heap_while(&mut empty_heap, |_| true).next().is_none());
    }
}
