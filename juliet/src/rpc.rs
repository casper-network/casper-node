//! RPC layer.
//!
//! Typically the outermost layer of the `juliet` stack is the RPC layer, which combines the
//! underlying IO and protocol primites into a convenient, type safe RPC system.

use std::{cell::OnceCell, collections::HashMap, sync::Arc, time::Duration};

use bytes::Bytes;

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc::Receiver, Notify},
};

use crate::{
    io::{CoreError, EnqueueError, Handle, IoCore, IoEvent, IoId, RequestHandle},
    protocol::{LocalProtocolViolation, ProtocolBuilder},
    ChannelId, Id,
};

#[derive(Default)]
pub struct RpcBuilder<const N: usize> {
    protocol: ProtocolBuilder<N>,
}

impl<const N: usize> RpcBuilder<N> {
    fn new(protocol: ProtocolBuilder<N>) -> Self {
        RpcBuilder { protocol }
    }

    /// Update the channel configuration for a given channel.
    pub fn build<R, W>(
        &self,
        reader: R,
        writer: W,
    ) -> (JulietRpcClient<N>, JulietRpcServer<N, R, W>) {
        todo!()
    }
}

/// Juliet RPC client.
///
/// The client is used to create new RPC calls.
pub struct JulietRpcClient<const N: usize> {
    // TODO
}

/// Juliet RPC Server.
///
/// The server's sole purpose is to handle incoming RPC calls.
pub struct JulietRpcServer<const N: usize, R, W> {
    core: IoCore<N, R, W>,
    handle: Handle,
    pending: HashMap<IoId, Arc<RequestGuardInner>>,
    new_requests: Receiver<(IoId, Arc<RequestGuardInner>)>,
}

#[derive(Debug)]
struct RequestGuardInner {
    /// The returned response of the request.
    outcome: OnceCell<Result<Option<Bytes>, RequestError>>,
    /// A notifier for when the result arrives.
    ready: Option<Notify>,
}

type RequestOutcome = Arc<OnceCell<Result<(), RequestError>>>;

pub struct JulietRpcRequestBuilder<const N: usize> {
    request_handle: RequestHandle<N>,
    channel: ChannelId,
    payload: Option<Bytes>,
    timeout: Duration, // TODO: Properly handle.
}

impl<const N: usize> JulietRpcClient<N> {
    /// Creates a new RPC request builder.
    ///
    /// The returned builder can be used to create a single request on the given channel.
    fn create_request(&self, channel: ChannelId) -> JulietRpcRequestBuilder<N> {
        todo!()
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
    async fn next_request(&mut self) -> Result<Option<IncomingRequest>, RpcServerError> {
        if let Some(event) = self.core.next_event().await? {
            match event {
                IoEvent::NewRequest {
                    channel,
                    id,
                    payload,
                } => Ok(Some(IncomingRequest {
                    channel,
                    id,
                    payload,
                    handle: Some(self.handle.clone()),
                })),
                IoEvent::RequestCancelled { channel, id } => todo!(),
                IoEvent::ReceivedResponse { io_id, payload } => todo!(),
                IoEvent::ReceivedCancellationResponse { io_id } => todo!(),
            }
        } else {
            Ok(None)
        }
    }
}

impl<const N: usize> JulietRpcRequestBuilder<N> {
    /// Sets the payload for the request.
    pub fn with_payload(mut self, payload: Bytes) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets the timeout for the request.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Schedules a new request on an outgoing channel.
    ///
    /// Blocks until space to store it is available.
    pub async fn queue_for_sending(mut self) -> RequestGuard {
        let outcome = OnceCell::new();

        let inner = match self
            .request_handle
            .enqueue_request(self.channel, self.payload)
            .await
        {
            Ok(io_id) => RequestGuardInner {
                outcome,
                ready: Some(Notify::new()),
            },
            Err(payload) => {
                outcome.set(Err(RequestError::RemoteClosed(payload)));
                RequestGuardInner {
                    outcome,
                    ready: None,
                }
            }
        };

        RequestGuard {
            inner: Arc::new(inner),
        }
    }

    /// Try to schedule a new request.
    ///
    /// Fails if local buffer is full.
    pub fn try_queue_for_sending(mut self) -> Result<RequestGuard, Self> {
        match self
            .request_handle
            .try_enqueue_request(self.channel, self.payload)
        {
            Ok(io_id) => Ok(RequestGuard {
                inner: Arc::new(RequestGuardInner {
                    outcome: OnceCell::new(),
                    ready: Some(Notify::new()),
                }),
            }),
            Err(EnqueueError::Closed(payload)) => {
                // Drop the payload, give a handle that is already "expired".
                Ok(RequestGuard::error(RequestError::RemoteClosed(payload)))
            }
            Err(EnqueueError::LocalProtocolViolation(violation)) => {
                Ok(RequestGuard::error(RequestError::Error(violation)))
            }
            Err(EnqueueError::BufferLimitHit(payload)) => Err(JulietRpcRequestBuilder {
                request_handle: self.request_handle,
                channel: self.channel,
                payload,
                timeout: self.timeout,
            }),
        }
    }
}

#[derive(Debug)]
pub enum RequestError {
    /// Remote closed, could not send.
    RemoteClosed(Option<Bytes>),
    /// Local timeout.
    TimedOut,
    /// Remote said "no".
    RemoteCancelled,
    /// Cancelled locally.
    Cancelled,
    /// API misuse
    Error(LocalProtocolViolation),
}

pub struct RequestGuard {
    inner: Arc<RequestGuardInner>,
}

impl RequestGuard {
    fn error(error: RequestError) -> Self {
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

    /// Cancels the request, causing it to not be sent if it is still in the queue.
    ///
    /// No response will be available for the request, any call to `wait_for_finish` will result in an error.
    pub fn cancel(self) {
        todo!()
    }

    /// Forgets the request was made.
    ///
    /// Any response will be accepted, but discarded.
    pub fn forget(self) {
        todo!()
    }

    /// Waits for the response to come back.
    pub async fn wait_for_response(self) -> Result<Option<Bytes>, RequestError> {
        todo!()
    }

    /// Waits for the response, non-blockingly.
    pub fn try_wait_for_response(self) -> Result<Result<Option<Bytes>, RequestError>, Self> {
        todo!()
    }

    /// Waits for the sending to complete.
    pub async fn wait_for_send(&mut self) {
        todo!()
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        todo!("on drop, cancel request")
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
