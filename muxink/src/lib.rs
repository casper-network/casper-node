//! Asynchronous multiplexing.
//!
//! The `muxink` crate allows building complex stream setups that multiplex, fragment, encode and
//! backpressure messages sent across asynchronous streams.
//!
//! # How to get started
//!
//! At the lowest level, the [`io::FrameReader`] and [`io::FrameWriter`] wrappers provide
//! [`Sink`](futures::Sink) and [`Stream`](futures::Stream) implementations on top of
//! [`AsyncRead`](futures::AsyncRead) and [`AsyncWrite`](futures::AsyncWrite) implementing types.
//! These can then be wrapped with any of types [`mux`]/[`demux`], [`fragmented`] or
//! [`backpressured`] to layer functionality on top.
//!
//! # Cancellation safety
//!
//! All streams and sinks constructed by combining types from this crate at least uphold the
//! following invariants:
//!
//! * [`SinkExt::send`](futures::SinkExt::send), [`SinkExt::send_all`](futures::SinkExt::send_all):
//!   Safe to cancel, although no guarantees are made whether an item was actually sent -- if the
//!   sink was still busy, it may not have been moved into the sink. The underlying stream will be
//!   left in a consistent state regardless.
//! * [`SinkExt::flush`](futures::SinkExt::flush): Safe to cancel.
//! * [`StreamExt::next`](futures::StreamExt::next): Safe to cancel. Cancelling it will not cause
//!   items to be lost upon construction of another [`next`](futures::StreamExt::next) future.

pub mod backpressured;
pub mod demux;
pub mod fragmented;
pub mod framing;
pub mod io;
pub mod mux;
#[cfg(test)]
pub mod testing;

use bytes::Buf;

/// Helper macro for returning a `Poll::Ready(Err)` eagerly.
///
/// Can be remove once `Try` is stabilized for `Poll`.
#[macro_export]
macro_rules! try_ready {
    ($ex:expr) => {
        match $ex {
            Err(e) => return Poll::Ready(Err(e.into())),
            Ok(v) => v,
        }
    };
}

/// A frame for stack allocated data.
#[derive(Debug)]
pub struct ImmediateFrame<A> {
    /// How much of the frame has been read.
    pos: usize,
    /// The actual value contained.
    value: A,
}

impl<A> ImmediateFrame<A> {
    #[inline]
    pub fn new(value: A) -> Self {
        Self { pos: 0, value }
    }
}

/// Implements conversion functions to immediate types for atomics like `u8`, etc.
macro_rules! impl_immediate_frame_le {
    ($t:ty) => {
        impl From<$t> for ImmediateFrame<[u8; ::std::mem::size_of::<$t>()]> {
            #[inline]
            fn from(value: $t) -> Self {
                ImmediateFrame::new(value.to_le_bytes())
            }
        }
    };
}

impl_immediate_frame_le!(u8);
impl_immediate_frame_le!(u16);
impl_immediate_frame_le!(u32);

impl<A> Buf for ImmediateFrame<A>
where
    A: AsRef<[u8]>,
{
    fn remaining(&self) -> usize {
        // Does not overflow, as `pos` is  `< .len()`.

        self.value.as_ref().len() - self.pos
    }

    fn chunk(&self) -> &[u8] {
        // Safe access, as `pos` is guaranteed to be `< .len()`.
        &self.value.as_ref()[self.pos..]
    }

    fn advance(&mut self, cnt: usize) {
        // This is the only function modifying `pos`, upholding the invariant of it being smaller
        // than the length of the data we have.
        self.pos = (self.pos + cnt).min(self.value.as_ref().len());
    }
}
