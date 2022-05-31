//! Stream multiplexing
//!
//! Multiplexes multiple sinks into a single one, allowing no more than one frame to be buffered for
//! each to avoid starvation or flooding.

use std::{
    mem,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{
    future::{BoxFuture, Fuse, FusedFuture},
    Future, FutureExt, Sink, SinkExt,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::ReusableBoxFuture;

use crate::{error::Error, ImmediateFrame};

pub type ChannelPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, F>;

/// A frame multiplexer.
///
/// Typically the multiplexer is not used directly, but used to spawn multiplexing handles.
struct Multiplexer<S> {
    sink: Arc<Mutex<Option<S>>>,
}

impl<S> Multiplexer<S> {
    /// Create a handle for a specific multiplexer channel on this multiplexer.
    pub fn get_channel_handle(self: Arc<Self>, channel: u8) -> MultiplexerHandle<S> {
        MultiplexerHandle {
            multiplexer: self.clone(),
            slot: channel,
            lock_future: todo!(),
            guard: None,
        }
    }
}

type SinkGuard<S> = OwnedMutexGuard<Option<S>>;

trait FuseFuture: Future + FusedFuture + Send {}
impl<T> FuseFuture for T where T: Future + FusedFuture + Send {}

type BoxFusedFuture<'a, T> = Pin<Box<dyn FuseFuture<Output = T> + Send + 'a>>;

struct MultiplexerHandle<S> {
    multiplexer: Arc<Multiplexer<S>>,
    slot: u8,
    // TODO: We ideally want to reuse the alllocated memory here,
    // mem::replace, then Box::Pin on it.

    // TODO NEW IDEA: Maybe we can create the lock future right away, but never poll it? Then use
    //                the `ReusableBoxFuture` and always create a new one right away? Need to check
    //                source of `lock`.
    lock_future: Box<dyn Future<Output = SinkGuard<S>> + Send + 'static>,
    guard: Option<SinkGuard<S>>,
}

impl<S> MultiplexerHandle<S> {
    fn assume_get_sink(&mut self) -> &mut S {
        match self.guard {
            Some(ref mut guard) => {
                let mref = guard.as_mut().expect("TODO: guard disappeard");
                mref
            }
            None => todo!("assumed sink, but no sink"),
        }
    }
}

impl<F, S> Sink<F> for MultiplexerHandle<S>
where
    S: Sink<ChannelPrefixedFrame<F>> + Unpin + Send + 'static,
    F: Buf,
{
    type Error = <S as Sink<ChannelPrefixedFrame<F>>>::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let guard = match self.guard {
            None => {
                // We do not hold the lock yet. If there is no future to acquire it, create one.
                if self.lock_fused {
                    let new_fut = self.multiplexer.sink.clone().lock_owned();

                    mem::replace(&mut self.lock_future, new_fut);
                    let fut = self.multiplexer.sink.clone().lock_owned().fused().boxed();
                    // TODO: mem::replace here?
                    self.lock_future = fut;
                }

                let fut = &mut self.lock_future;

                let guard = match fut.poll_unpin(cx) {
                    Poll::Ready(guard) => {
                        // Lock acquired. Store it and clear the future, so we don't poll it again.
                        self.guard.insert(guard)
                    }
                    Poll::Pending => return Poll::Pending,
                };

                guard
            }
            Some(ref mut guard) => guard,
        };

        // Now that we hold the lock, poll the sink.
        self.assume_get_sink().poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let prefixed = ImmediateFrame::from(self.slot).chain(item);
        self.assume_get_sink().start_send_unpin(prefixed)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Obtain the flush result, then release the sink lock.
        match self.assume_get_sink().poll_flush_unpin(cx) {
            Poll::Ready(Ok(())) => {
                // Acquire wait list lock to update it.
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(_)) => {
                todo!("handle error")
            }

            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Simply close? Note invariants, possibly checking them in debug mode.
        todo!()
    }
}
