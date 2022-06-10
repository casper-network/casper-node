//! Stream multiplexing
//!
//! Multiplexes multiple sinks into a single one, allowing no more than one frame to be buffered for
//! each. Up to 256 channels are supported, being encoded with a leading byte on the underlying
//! downstream.
//!
//! ## Fairness
//!
//! Multiplexing is fair per handle, that is every handle is eventually guaranteed to receive a slot
//! for sending on the underlying sink. Under maximal contention, every `MultiplexerHandle` will
//! receive `1/n` of the slots, with `n` being the total number of multiplexers, with no handle
//! being able to send more than twice without all other waiting handles receiving a slot.

use std::{
    pin::Pin,
    sync::Arc,
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
/// Can be remove once `Try` is stabilized for `Poll`.
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
    /// Any item sent via this handle's `Sink` implementation will be sent on the given channel.
    ///
    /// It is valid to have multiple handles for the same channel.
    pub fn create_channel_handle(&self, channel: u8) -> MultiplexerHandle<S>
    where
        S: Send + 'static,
    {
        MultiplexerHandle {
            sink: self.sink.clone(),
            channel,
            lock_future: ReusableBoxFuture::new(mk_lock_future(self.sink.clone())),
            sink_guard: None,
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
pub struct MultiplexerHandle<S> {
    /// The sink shared across the multiplexer and all its handles.
    sink: Arc<Mutex<Option<S>>>,
    /// Channel ID assigned to this handle.
    channel: u8,
    /// The future locking the shared sink.
    // Note: To avoid frequent heap allocations, a single box is reused for every lock this handle
    //       needs to acquire, whcich is on every sending of an item via `Sink`.
    //
    //       This relies on the fact that merely instantiating the locking future (via
    //       `mk_lock_future`) will not do anything before the first poll (see
    //       `tests::ensure_creating_lock_acquisition_future_is_side_effect_free`).
    lock_future: ReusableBoxFuture<'static, SinkGuard<S>>,
    /// A potential acquired guard for the underlying sink.
    ///
    /// Proper acquisition and dropping of the guard is dependent on callers obeying the sink
    /// protocol. A call to `poll_ready` will commence and ultimately complete guard acquisition.
    ///
    /// A [`Poll::Ready`] return value from either `poll_flush` or `poll_close` will release it.
    sink_guard: Option<SinkGuard<S>>,
}

impl<S> MultiplexerHandle<S>
where
    S: Send + 'static,
{
    /// Retrieve the shared sink, assuming a guard is held.
    ///
    /// Returns `Err(Error::MultiplexerClosed)` if the sink has been removed.
    ///
    /// # Panics
    ///
    /// If no guard is held in `self.guard`, panics.
    fn assume_get_sink<F>(&mut self) -> Result<&mut S, Error<<S as Sink<F>>::Error>>
    where
        S: Sink<F>,
        <S as Sink<F>>::Error: std::error::Error,
    {
        match self.sink_guard {
            Some(ref mut guard) => match guard.as_mut() {
                Some(sink) => Ok(sink),
                None => Err(Error::MultiplexerClosed),
            },
            None => {
                panic!("assume_get_sink called without holding a sink. this is a bug")
            }
        }
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

        // At this point we have acquired the lock, now our only job is to stuff data into the sink.
        try_ready!(sink_guard.as_mut().ok_or(Error::MultiplexerClosed))
            .poll_ready_unpin(cx)
            .map_err(Error::Sink)
    }

    fn start_send(mut self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let prefixed = ImmediateFrame::from(self.channel).chain(item);

        self.assume_get_sink()?
            .start_send_unpin(prefixed)
            .map_err(Error::Sink)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let sink = try_ready!(self.assume_get_sink());

        let outcome = ready!(sink.poll_flush_unpin(cx));
        self.sink_guard = None;
        Poll::Ready(outcome.map_err(Error::Sink))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let sink = try_ready!(self.assume_get_sink());

        let outcome = ready!(sink.poll_close_unpin(cx));
        self.sink_guard = None;

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
            .send(Bytes::from(&b"Seceond"[..]))
            .now_or_never()
            .unwrap()
            .unwrap_err();
        assert!(matches!(outcome, Error::MultiplexerClosed));
    }

    #[test]
    fn cancelled_send_does_not_deadlock_multiplexer() {
        let sink = Arc::new(TestingSink::new());
        let muxer = Multiplexer::new(sink.clone().into_ref());

        sink.set_clogged(true);
        let mut chan_0 = muxer.create_channel_handle(0);

        assert!(chan_0
            .send(Bytes::from(&b"zero"[..]))
            .now_or_never()
            .is_none());

        // At this point, we have cancelled a send that was in progress due to the sink not having
        // finished. The sink will finish eventually, but has not been polled to completion.
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
        assert!(chan_0
            .send(Bytes::from(&b"zero"[..]))
            .now_or_never()
            .is_none());

        // The data has already arrived (it's a clog, not a plug):
        assert_eq!(sink.get_contents(), b"\x00zero");

        println!("zero sent");
        // The other two channels are sending in order.
        let send_1 = tokio::spawn(async move {
            println!("begin chan_1 sending");
            chan_1.send(Bytes::from(&b"one"[..])).await.unwrap();
            println!("done chan_1 sending");
        });
        println!("send_1 spawned");

        // Yield, ensuring that `one` is in queue acquiring the lock first (since it is not plugged,
        // it should enter the lock wait queue).

        tokio::task::yield_now().await;

        let send_2 =
            tokio::spawn(async move { chan_2.send(Bytes::from(&b"two"[..])).await.unwrap() });
        println!("send_2 spawned");
        tokio::task::yield_now().await;

        // Unclog.
        sink.set_clogged(false);
        println!("unclogged");

        // Both should finish with the unclogged sink.
        send_2.await.unwrap();
        println!("send_2 finished");
        send_1.await.unwrap();
        println!("send_1 finished");

        // The final result should be in order.
        assert_eq!(sink.get_contents(), b"\x00zero\x01one\x02two");
    }
}
