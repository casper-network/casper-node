#![allow(dead_code, unused)]

//! RPC layer.
//!
//! Typically the outermost layer of the `juliet` stack is the RPC layer, which combines the
//! underlying IO and protocol primites into a convenient, type safe RPC system.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures::Stream;

use crate::ChannelId;

/// Creates a new set of RPC client (for making RPC calls) and RPC server (for handling calls).
pub fn make_rpc<T>(transport: T) -> (JulietRpcClient, JulietRpcServer) {
    // TODO: Consider allowing for zero-to-many clients to be created.
    todo!()
}

/// Juliet RPC client.
///
/// The client is used to create new RPC calls.
pub struct JulietRpcClient {
    // TODO
}

/// Juliet RPC Server.
///
/// The server's sole purpose is to handle incoming RPC calls.
pub struct JulietRpcServer {
    // TODO
}

pub struct JulietRpcRequestBuilder {
    // TODO
}

impl JulietRpcClient {
    /// Creates a new RPC request builder.
    ///
    /// The returned builder can be used to create a single request on the given channel.
    fn create_request(&self, channel: ChannelId) -> JulietRpcRequestBuilder {
        todo!()
    }
}

pub struct IncomingRequest {
    // TODO
}

pub enum RpcServerError {
    // TODO
}

impl Stream for JulietRpcServer {
    type Item = Result<IncomingRequest, RpcServerError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

pub struct RequestHandle;

impl JulietRpcRequestBuilder {
    /// Sets the payload for the request.
    pub fn with_payload(self, payload: Bytes) -> Self {
        todo!()
    }

    /// Sets the timeout for the request.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        todo!()
    }

    /// Schedules a new request on an outgoing channel.
    ///
    /// Blocks until space to store it is available.
    pub async fn queue_for_sending(self) -> RequestHandle {
        todo!()
    }

    /// Try to schedule a new request.
    ///
    /// Fails if local buffer is exhausted.
    pub fn try_queue_for_sending(self) -> Result<RequestHandle, Self> {
        todo!()
    }
}

pub enum RequestError {
    /// Remote closed due to some error, could not send.
    RemoteError,
    /// Local timeout.
    TimedOut,
    /// Remote said "no".
    RemoteCancelled,
    /// Cancelled locally.
    Cancelled,
    /// API misuse
    Error,
}

// Note: On drop, `RequestHandle` cancels itself.
impl RequestHandle {
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

impl Drop for RequestHandle {
    fn drop(&mut self) {
        todo!("on drop, cancel request")
    }
}

impl IncomingRequest {
    /// Returns a reference to the payload, if any.
    pub fn payload(&self) -> &Option<Bytes> {
        todo!()
    }

    /// Returns a reference to the payload, if any.
    ///
    /// Typically used in conjunction with [`Option::take()`].
    pub fn payload_mut(&self) -> &mut Option<Bytes> {
        todo!()
    }

    /// Enqueue a response to be sent out.
    pub fn respond(self, payload: Bytes) {
        todo!()
    }

    /// Cancel the request.
    ///
    /// This will cause a cancellation to be sent back.
    pub fn cancel(self) {
        todo!()
    }
}

impl Drop for IncomingRequest {
    fn drop(&mut self) {
        todo!("send cancel response")
    }
}
