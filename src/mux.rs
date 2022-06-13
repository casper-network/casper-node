//! Stream multiplexing
//!
//! Multiplexes multiple sinks into a single one, without buffering any items. Up to 256 channels
//! are supported, each item sent on a specific channel will be forwarded with a 1-byte prefix
//! indicating the channel.
//!
//! ## Fairness
//!
//! Multiplexing is fair per handle, that is every handle is eventually guaranteed to receive a slot
//! for sending on the underlying sink. Under maximal contention, every `MultiplexerHandle` will
//! receive `1/n` of the slots, with `n` being the total number of multiplexers, with no handle
//! being able to send more than twice without all other waiting handles receiving a slot.
//!
//! ## Locking
//!
//! Sending and flushing an item each requires a separate lock acquisition, as the lock is released
//! after each `start_send` operation. This in turn means that a [`SinkExt::send_all`] call will not
//! hold the underlying output sink hostage until all items are send.

use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{ready, FutureExt, Sink, SinkExt};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::ReusableBoxFuture;

use crate::{error::Error, ImmediateFrame};

pub type ChannelPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, F>;

/// Helper macro for returning a `Poll::Ready(Err)` eagerly.
///
/// Can be removed once `Try` is stabilized for `Poll`.
macro_rules! try_ready {
    ($ex:expr) => {
        match $ex {
            Err(e) => return Poll::Ready(Err(e.into())),
            Ok(v) => v,
        }
    };
}

/// A frame multiplexer.
///
/// A multiplexer is not used directly, but used to spawn multiplexing handles.
pub struct Multiplexer<S> {
    /// The shared sink for output.
    sink: Arc<Mutex<Option<S>>>,
}

impl<S> Multiplexer<S> {
    /// Creates a new multiplexer with the given sink.
    pub fn new(sink: S) -> Self {
        Self {
            sink: Arc::new(Mutex::new(Some(sink))),
        }
    }

    /// Create a handle for a specific multiplexer channel on this multiplexer.
    ///
    /// Any item sent via this handle's `Sink` implementation will be sent on the given channel by
    /// prefixing with the channel identifier (see module documentation).
    ///
    /// It is valid to have multiple handles for the same channel.
    ///
    /// # Correctness and cancellation safety
    ///
    /// Since a handle may hold a lock on the shared sink, additional invariants that must be upheld
    /// by the calling tasks:
    ///
    /// * Every call to `Sink::poll_ready` returning `Poll::Pending` **must** be repeated until
    ///   `Poll::Ready` is returned or followed by a drop of the handle.
    /// * Every call to `Sink::poll_ready` returning `Poll::Ready` **must** be followed by a call to
    ///   `Sink::start_send` or a drop of the handle.
    /// * Every call to `Sink::poll_flush` returning `Poll::Pending` must be repeated until
    ///   `Poll::Ready` is returned or followed by a drop of the handle.
    /// * Every call to `Sink::poll_close` returning `Poll::Pending` must be repeated until
    ///   `Poll::Ready` is returned or followed by a drop of the handle.
    ///
    /// As a result **the `SinkExt::send`, `SinkExt::send_all`, `SinkExt::flush` and
    /// `SinkExt::close` methods of any chain of sinks involving a `Multiplexer` is not cancellation
    /// safe**.
    pub fn create_channel_handle(&self, channel: u8) -> MultiplexerHandle<S>
    where
        S: Send + 'static,
    {
        MultiplexerHandle {
            sink: self.sink.clone(),
            send_count: Arc::new(AtomicUsize::new(0)),
            channel,
            lock_future: ReusableBoxFuture::new(mk_lock_future(self.sink.clone())),
            sink_guard: None,
            highest_flush: Arc::new(AtomicUsize::new(0)),
            last_send: None,
        }
    }

    /// Deconstructs the multiplexer into its sink.
    ///
    /// This function will block until outstanding writes to the underlying sink have completed. Any
    /// handle to this multiplexer will be closed afterwards.
    pub fn into_inner(self) -> S {
        self.sink
            .blocking_lock()
            .take()
            // This function is the only one ever taking out of the `Option<S>` and it consumes the
            // only `Multiplexer`, thus we can always expect a `Some` value here.
            .expect("did not expect sink to be missing")
    }
}

/// A guard of a protected sink.
type SinkGuard<S> = OwnedMutexGuard<Option<S>>;

/// Helper function to create a locking future.
///
/// It is important to always return a same-sized future when replacing futures using
/// `ReusableBoxFuture`. For this reason, lock futures are only ever created through this helper
/// function.
fn mk_lock_future<S>(
    sink: Arc<Mutex<Option<S>>>,
) -> impl futures::Future<Output = tokio::sync::OwnedMutexGuard<Option<S>>> {
    sink.lock_owned()
}

/// A handle to a multiplexer.
///
/// A handle is bound to a specific channel, see [`Multiplexer::create_channel_handle`] for details.
///
/// Closing a handle will close the underlying multiplexer stream. To only "close" a specific
/// channel, flush the handle and drop it.
pub struct MultiplexerHandle<S> {
    /// The sink shared across the multiplexer and all its handles.
    sink: Arc<Mutex<Option<S>>>,
    /// The number of items sent to the underlying sink.
    send_count: Arc<AtomicUsize>,
    /// Highest `send_count` that has been flushed.
    highest_flush: Arc<AtomicUsize>,
    /// The send count at which our last enqueued data was sent.
    last_send: Option<usize>,
    /// Channel ID assigned to this handle.
    channel: u8,
    /// The future locking the shared sink.
    // Note: To avoid frequent heap allocations, a single box is reused for every lock this handle
    //       needs to acquire, which is on every sending of an item via `Sink`.
    //
    //       This relies on the fact that merely instantiating the locking future (via
    //       `mk_lock_future`) will not do anything before the first poll (see
    //       `tests::ensure_creating_lock_acquisition_future_is_side_effect_free`).
    lock_future: ReusableBoxFuture<'static, SinkGuard<S>>,
    /// A potential acquired guard for the underlying sink.
    ///
    /// Proper acquisition and dropping of the guard is dependent on callers obeying the sink
    /// protocol and the invariants specified in the [`Multiplexer::create_channel_handle`]
    /// documentation.
    ///
    /// A [`Poll::Ready`] return value from either `poll_flush` or `poll_close` or a call to
    /// `start_send` will release the guard.
    sink_guard: Option<SinkGuard<S>>,
}

impl<S> MultiplexerHandle<S>
where
    S: Send + 'static,
{
    /// Acquire or return a guard on the sink lock.
    ///
    /// Helper function for lock acquisition:
    ///
    /// * If the lock is already obtained, returns `Ready(guard)`.
    /// * If the lock has not been obtained, attempts to poll the locking future, either returning
    ///   `Pending` or `Ready(guard)`.
    fn acquire_lock(&mut self, cx: &mut Context<'_>) -> Poll<&mut SinkGuard<S>> {
        let sink_guard = match self.sink_guard {
            None => {
                // We do not hold the guard at the moment, so attempt to acquire it.
                match self.lock_future.poll_unpin(cx) {
                    Poll::Ready(guard) => {
                        // It is our turn: Save the guard and prepare another locking future for
                        // later, which will not attempt to lock until first polled.
                        let sink = self.sink.clone();
                        self.lock_future.set(mk_lock_future(sink));
                        self.sink_guard.insert(guard)
                    }
                    Poll::Pending => {
                        // The lock could not be acquired yet.
                        return Poll::Pending;
                    }
                }
            }
            Some(ref mut guard) => guard,
        };
        Poll::Ready(sink_guard)
    }
}

impl<F, S> Sink<F> for MultiplexerHandle<S>
where
    S: Sink<ChannelPrefixedFrame<F>> + Unpin + Send + 'static,
    F: Buf,
    <S as Sink<ChannelPrefixedFrame<F>>>::Error: std::error::Error,
{
    type Error = Error<<S as Sink<ChannelPrefixedFrame<F>>>::Error>;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let sink_guard = ready!(self.acquire_lock(cx));

        // We have acquired the lock, now our job is to wait for the sink to become ready.
        try_ready!(sink_guard.as_mut().ok_or(Error::MultiplexerClosed))
            .poll_ready_unpin(cx)
            .map_err(Error::Sink)
    }

    fn start_send(mut self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let prefixed = ImmediateFrame::from(self.channel).chain(item);

        // We take the guard here, so that early exits due to errors will free the lock.
        let mut guard = match self.sink_guard.take() {
            Some(guard) => guard,
            None => {
                panic!("protocol violation - `start_send` called before `poll_ready`");
            }
        };

        let sink = match guard.as_mut() {
            Some(sink) => sink,
            None => {
                return Err(Error::MultiplexerClosed);
            }
        };

        sink.start_send_unpin(prefixed).map_err(Error::Sink)?;

        // Item is enqueued, increase the send count.
        let last_send = self.send_count.fetch_add(1, Ordering::SeqCst) + 1;
        self.last_send = Some(last_send);

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Check if our last message was already flushed, this saves us some needless locking.
        let last_send = if let Some(last_send) = self.last_send {
            if self.highest_flush.load(Ordering::SeqCst) >= last_send {
                // Someone else flushed the sink for us.
                self.last_send = None;
                self.sink_guard.take();
                return Poll::Ready(Ok(()));
            }

            last_send
        } else {
            // There was no data that we are waiting to flush still.
            self.sink_guard.take();
            return Poll::Ready(Ok(()));
        };

        // At this point we know that we have to flush, and for that we need the lock.
        let sink_guard = ready!(self.acquire_lock(cx));

        let outcome = match sink_guard.as_mut() {
            Some(sink) => {
                // We have the lock, so try to flush.
                ready!(sink.poll_flush_unpin(cx))
            }
            None => {
                self.sink_guard.take();
                return Poll::Ready(Err(Error::MultiplexerClosed));
            }
        };

        if outcome.is_ok() {
            self.highest_flush.fetch_max(last_send, Ordering::SeqCst);
            self.last_send.take();
        }

        // Release lock.
        self.sink_guard.take();

        Poll::Ready(outcome.map_err(Error::Sink))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let sink_guard = ready!(self.acquire_lock(cx));

        let outcome = match sink_guard.as_mut() {
            Some(sink) => {
                ready!(sink.poll_close_unpin(cx))
            }
            None => {
                // Closing an underlying closed multiplexer has no effect.
                self.sink_guard.take();
                return Poll::Ready(Ok(()));
            }
        };

        // Release lock.
        self.sink_guard.take();

        Poll::Ready(outcome.map_err(Error::Sink))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use futures::{FutureExt, SinkExt};
    use tokio::sync::Mutex;

    use crate::{
        error::Error,
        tests::{collect_bufs, TestingSink},
    };

    use super::{ChannelPrefixedFrame, Multiplexer};

    #[test]
    fn ensure_creating_lock_acquisition_future_is_side_effect_free() {
        // This test ensures an assumed property in the multiplexer's sink implementation, namely
        // that calling the `.lock_owned()` function does not affect the lock before being polled.

        let mutex: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

        // Instantiate a locking future without polling it.
        let lock_fut = mutex.clone().lock_owned();

        // Creates a second locking future, which we will poll immediately. It should return ready.
        assert!(mutex.lock_owned().now_or_never().is_some());

        // To prove that the first one also worked, poll it as well.
        assert!(lock_fut.now_or_never().is_some());
    }

    #[test]
    fn mux_lifecycle() {
        let output: Vec<ChannelPrefixedFrame<Bytes>> = Vec::new();
        let muxer = Multiplexer::new(output);

        let mut chan_0 = muxer.create_channel_handle(0);
        let mut chan_1 = muxer.create_channel_handle(1);

        assert!(chan_1
            .send(Bytes::from(&b"Hello"[..]))
            .now_or_never()
            .is_some());
        assert!(chan_0
            .send(Bytes::from(&b"World"[..]))
            .now_or_never()
            .is_some());

        let output = collect_bufs(muxer.into_inner());
        assert_eq!(output, b"\x01Hello\x00World")
    }

    #[test]
    fn into_inner_invalidates_handles() {
        let output: Vec<ChannelPrefixedFrame<Bytes>> = Vec::new();
        let muxer = Multiplexer::new(output);

        let mut chan_0 = muxer.create_channel_handle(0);

        assert!(chan_0
            .send(Bytes::from(&b"Sample"[..]))
            .now_or_never()
            .is_some());

        muxer.into_inner();

        let outcome = chan_0
            .send(Bytes::from(&b"Second"[..]))
            .now_or_never()
            .unwrap()
            .unwrap_err();
        assert!(matches!(outcome, Error::MultiplexerClosed));
    }

    #[test]
    fn cancelled_send_does_not_deadlock_multiplexer_if_handle_dropped() {
        let sink = Arc::new(TestingSink::new());
        let muxer = Multiplexer::new(sink.clone().into_ref());

        sink.set_clogged(true);
        let mut chan_0 = muxer.create_channel_handle(0);

        assert!(chan_0
            .send(Bytes::from(&b"zero"[..]))
            .now_or_never()
            .is_none());

        // At this point, we have cancelled a send that was in progress due to the sink not having
        // finished. The sink will finish eventually, but has not been polled to completion, which
        // means the lock is still engaged. Dropping the handle resolves this.
        drop(chan_0);

        // Unclog the sink - a fresh handle should be able to continue.
        sink.set_clogged(false);

        let mut chan_0 = muxer.create_channel_handle(1);
        assert!(chan_0
            .send(Bytes::from(&b"one"[..]))
            .now_or_never()
            .is_some());
    }

    #[tokio::test]
    async fn concurrent_sending() {
        let sink = Arc::new(TestingSink::new());
        let muxer = Multiplexer::new(sink.clone().into_ref());

        // Clog the sink for now.
        sink.set_clogged(true);

        let mut chan_0 = muxer.create_channel_handle(0);
        let mut chan_1 = muxer.create_channel_handle(1);
        let mut chan_2 = muxer.create_channel_handle(2);

        // Channel zero has a long send going on.
        let send_0 =
            tokio::spawn(async move { chan_0.send(Bytes::from(&b"zero"[..])).await.unwrap() });
        tokio::task::yield_now().await;

        // The data has already arrived (it's a clog, not a plug):
        assert_eq!(sink.get_contents(), b"\x00zero");

        // The other two channels are sending in order.
        let send_1 = tokio::spawn(async move {
            chan_1.send(Bytes::from(&b"one"[..])).await.unwrap();
        });

        // Yield, ensuring that `one` is in queue acquiring the lock first (since it is not plugged,
        // it should enter the lock wait queue).

        tokio::task::yield_now().await;

        let send_2 =
            tokio::spawn(async move { chan_2.send(Bytes::from(&b"two"[..])).await.unwrap() });

        tokio::task::yield_now().await;

        // Unclog, this causes the first write to finish and others to follow.
        sink.set_clogged(false);

        // All should finish with the unclogged sink.
        send_2.await.unwrap();
        send_0.await.unwrap();
        send_1.await.unwrap();

        // The final result should be in order.
        assert_eq!(sink.get_contents(), b"\x00zero\x01one\x02two");
    }
}
