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
    collections::HashMap,
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
/// The client is used to create new RPC calls through [`JulietRpcClient::create_request`].
#[derive(Clone, Debug)]
pub struct JulietRpcClient<const N: usize> {
    new_request_sender: UnboundedSender<NewOutgoingRequest>,
    request_handle: RequestHandle<N>,
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
    core: IoCore<N, R, W>,
    handle: Handle,
    pending: HashMap<IoId, Arc<RequestGuardInner>>,
    new_requests_receiver: UnboundedReceiver<NewOutgoingRequest>,
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
            timeout: None,
        }
    }
}

/// An error produced by the RPC error.
#[derive(Debug, Error)]
pub enum RpcServerError {
    /// An [`IoCore`] error.
    #[error(transparent)]
    CoreError(#[from] CoreError),
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
            tokio::select! {
                biased;

                opt_new_request = self.new_requests_receiver.recv() => {
                    if let Some(NewOutgoingRequest { ticket, guard, payload }) = opt_new_request {
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

        while let Ok(NewOutgoingRequest {
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
    ///
    /// **TODO**: Currently the timeout feature is not implemented.
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

        match self.client.new_request_sender.send(NewOutgoingRequest {
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
///
/// Describes the reason a request did not yield a response.
#[derive(Clone, Debug, Error)]
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
/// [`try_wait_for_response`](RequestGuard::try_wait_for_response).
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
    pub fn try_wait_for_response(self) -> Result<Result<Option<Bytes>, RequestError>, Self> {
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

    use crate::{
        io::IoCoreBuilder, protocol::ProtocolBuilder, rpc::RpcBuilder, ChannelConfiguration,
        ChannelId,
    };

    use super::{JulietRpcClient, JulietRpcServer};

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

    #[tokio::test]
    async fn basic_smoke_test() {
        let builder = RpcBuilder::new(IoCoreBuilder::new(
            ProtocolBuilder::<2>::with_default_channel_config(
                ChannelConfiguration::new()
                    .with_max_request_payload_size(1024)
                    .with_max_response_payload_size(1024),
            ),
        ));

        let (client, server) = setup_peers(builder);

        // Spawn an echo-server.
        tokio::spawn(async move {
            let (rpc_client, mut rpc_server) = server;

            while let Some(req) = rpc_server
                .next_request()
                .await
                .expect("error receiving request")
            {
                println!("recieved {}", req);
                let payload = req.payload().clone();
                req.respond(payload);
            }

            drop(rpc_client);
        });

        let (rpc_client, mut rpc_server) = client;

        // Run the background process for the client.
        tokio::spawn(async move {
            while let Some(inc) = rpc_server
                .next_request()
                .await
                .expect("client rpc_server error")
            {
                panic!("did not expect to receive {:?} on client", inc);
            }
        });

        let payload = Bytes::from(&b"foobar"[..]);

        let response = rpc_client
            .create_request(ChannelId::new(0))
            .with_payload(payload.clone())
            .queue_for_sending()
            .await
            .wait_for_response()
            .await
            .expect("request failed");

        assert_eq!(response, Some(payload));
    }
}
