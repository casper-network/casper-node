//! RPC layer.
//!
//! The outermost layer of the `juliet` stack, combines the underlying IO and protocol primites into
//! a convenient, type safe RPC system.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use bytes::Bytes;

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Notify,
    },
};

use crate::{
    io::{
        CoreError, EnqueueError, Handle, IoCore, IoCoreBuilder, IoEvent, IoId, RequestHandle,
        RequestTicket, ReservationError,
    },
    protocol::LocalProtocolViolation,
    ChannelId, Id,
};

/// Builder for a new RPC interface.
pub struct RpcBuilder<const N: usize> {
    /// The IO core builder used.
    core: IoCoreBuilder<N>,
}

impl<const N: usize> RpcBuilder<N> {
    /// Constructs a new RPC builder.
    ///
    /// The builder can be reused to create instances for multiple connections.
    pub fn new(core: IoCoreBuilder<N>) -> Self {
        RpcBuilder { core }
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
        };
        let server = JulietRpcServer {
            core,
            handle: core_handle.downgrade(),
            pending: Default::default(),
            new_requests_receiver,
        };

        (client, server)
    }
}

/// Juliet RPC client.
///
/// The client is used to create new RPC calls.
pub struct JulietRpcClient<const N: usize> {
    new_request_sender: UnboundedSender<NewRequest>,
    request_handle: RequestHandle<N>,
}

pub struct JulietRpcRequestBuilder<'a, const N: usize> {
    client: &'a JulietRpcClient<N>,
    channel: ChannelId,
    payload: Option<Bytes>,
    timeout: Option<Duration>,
}

/// Juliet RPC Server.
///
/// The server's sole purpose is to handle incoming RPC calls.
pub struct JulietRpcServer<const N: usize, R, W> {
    core: IoCore<N, R, W>,
    handle: Handle,
    pending: HashMap<IoId, Arc<RequestGuardInner>>,
    new_requests_receiver: UnboundedReceiver<NewRequest>,
}

struct NewRequest {
    ticket: RequestTicket,
    guard: Arc<RequestGuardInner>,
    payload: Option<Bytes>,
}

#[derive(Debug)]
struct RequestGuardInner {
    /// The returned response of the request.
    outcome: OnceLock<Result<Option<Bytes>, RequestError>>,
    /// A notifier for when the result arrives.
    ready: Option<Notify>,
}

impl RequestGuardInner {
    fn new() -> Self {
        RequestGuardInner {
            outcome: OnceLock::new(),
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
            timeout: None,
        }
    }
}

#[derive(Debug, Error)]

pub enum RpcServerError {
    #[error(transparent)]
    CoreError(#[from] CoreError),
}

impl<const N: usize, R, W> JulietRpcServer<N, R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub async fn next_request(&mut self) -> Result<Option<IncomingRequest>, RpcServerError> {
        loop {
            tokio::select! {
                biased;

                opt_new_request = self.new_requests_receiver.recv() => {
                    if let Some(NewRequest { ticket, guard, payload }) = opt_new_request {
                        match self.handle.enqueue_request(ticket, payload) {
                            Ok(io_id) => {
                                // The request will be sent out, store it in our pending map.
                                self.pending.insert(io_id, guard);
                            },
                            Err(payload) => {
                                // Failed to send -- time to shut down.
                                guard.set_and_notify(Err(RequestError::RemoteClosed(payload)))
                            }
                        }
                    } else {
                        // The client has been dropped, time for us to shut down as well.
                        return Ok(None);
                    }
                }

                opt_event = self.core.next_event() => {
                    if let Some(event) = opt_event? {
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
}

impl<const N: usize, R, W> Drop for JulietRpcServer<N, R, W> {
    fn drop(&mut self) {
        // When the server is dropped, ensure all waiting requests are informed.

        self.new_requests_receiver.close();

        for (_io_id, guard) in self.pending.drain() {
            guard.set_and_notify(Err(RequestError::Shutdown));
        }

        while let Ok(NewRequest {
            ticket: _,
            guard,
            payload,
        }) = self.new_requests_receiver.try_recv()
        {
            guard.set_and_notify(Err(RequestError::RemoteClosed(payload)))
        }
    }
}

impl<'a, const N: usize> JulietRpcRequestBuilder<'a, N> {
    /// Sets the payload for the request.
    pub fn with_payload(mut self, payload: Bytes) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets the timeout for the request.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Schedules a new request on an outgoing channel.
    ///
    /// Blocks until space to store it is available.
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
    pub fn try_queue_for_sending(self) -> Option<RequestGuard> {
        let ticket = match self.client.request_handle.try_reserve_request(self.channel) {
            Ok(ticket) => ticket,
            Err(ReservationError::Closed) => {
                return Some(RequestGuard::new_error(RequestError::RemoteClosed(
                    self.payload,
                )));
            }
            Err(ReservationError::NoBufferSpaceAvailable) => {
                return None;
            }
        };

        Some(self.do_enqueue_request(ticket))
    }

    #[inline(always)]
    fn do_enqueue_request(self, ticket: RequestTicket) -> RequestGuard {
        let inner = Arc::new(RequestGuardInner::new());

        match self.client.new_request_sender.send(NewRequest {
            ticket,
            guard: inner.clone(),
            payload: self.payload,
        }) {
            Ok(()) => RequestGuard { inner },
            Err(send_err) => {
                RequestGuard::new_error(RequestError::RemoteClosed(send_err.0.payload))
            }
        }
    }
}

/// An RPC request error.
#[derive(Clone, Debug, Error)]
pub enum RequestError {
    /// Remote closed, could not send.
    #[error("remote closed connection before request could be sent")]
    RemoteClosed(Option<Bytes>),
    /// Sent, but never received a reply.
    #[error("never received reply before remote closed connection")]
    Shutdown,
    /// Local timeout.
    #[error("request timed out ")]
    TimedOut,
    /// Remote said "no".
    #[error("remote cancelled our request")]
    RemoteCancelled,
    /// Cancelled locally.
    #[error("request cancelled locally")]
    Cancelled,
    /// API misuse
    #[error("API misused or other internal error")]
    Error(LocalProtocolViolation),
}

#[derive(Debug)]
#[must_use = "dropping the request guard will immediately cancel the request"]
pub struct RequestGuard {
    inner: Arc<RequestGuardInner>,
}

impl RequestGuard {
    fn new_error(error: RequestError) -> Self {
        let outcome = OnceLock::new();
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

    /// Cancels the request, causing it to not be sent if it is still in the queue.
    ///
    /// No response will be available for the request, any call to `wait_for_finish` will result in an error.
    pub fn cancel(mut self) {
        self.do_cancel();

        self.forget()
    }

    fn do_cancel(&mut self) {
        // TODO: Implement actual sending of the cancellation.
    }

    /// Forgets the request was made.
    ///
    /// Any response will be accepted, but discarded.
    pub fn forget(self) {
        // TODO: Implement eager cancellation locally, potentially removing this request from the
        //       outbound queue.
    }

    /// Waits for the response to come back.
    pub async fn wait_for_response(self) -> Result<Option<Bytes>, RequestError> {
        // Wait for notification.
        if let Some(ref ready) = self.inner.ready {
            ready.notified().await;
        }

        self.take_inner()
    }

    /// Waits for the response, non-blockingly.
    pub fn try_wait_for_response(self) -> Result<Result<Option<Bytes>, RequestError>, Self> {
        if self.inner.outcome.get().is_some() {
            Ok(self.take_inner())
        } else {
            Err(self)
        }
    }

    fn take_inner(self) -> Result<Option<Bytes>, RequestError> {
        // TODO: Best to move `Notified` + `OnceCell` into a separate struct for testing and upholding
        // these invariants, avoiding the extra clones.

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
/// [`IncomingRequest::respond()`] methods. If dropped, [`IncomingRequest::cancel()`] is called
/// automatically.
#[derive(Debug)]
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

impl IncomingRequest {
    /// Returns a reference to the payload, if any.
    #[inline(always)]
    pub fn payload(&self) -> &Option<Bytes> {
        &self.payload
    }

    /// Returns a reference to the payload, if any.
    ///
    /// Typically used in conjunction with [`Option::take()`].
    #[inline(always)]
    pub fn payload_mut(&mut self) -> &mut Option<Bytes> {
        &mut self.payload
    }

    /// Enqueue a response to be sent out.
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
                    EnqueueError::LocalProtocolViolation(_) => {
                        todo!("what to do with this?")
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
                    EnqueueError::LocalProtocolViolation(_) => {
                        todo!("what to do with this?")
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
