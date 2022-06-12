//! Immediate (small/fixed size) item sink and stream.
//!
//! `ImmediateSink` allows sending items for which `Into<ImmediateFrameSink<_>>` is
//! implemented. Typically this is true for small atomic types like `u32`, which are encoded as
//! little endian in throughout this crate.
//!
//! No additional headers are added, as immediate values are expected to be of fixed size.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, SinkExt};

use crate::ImmediateFrame;

/// Sink for immediate values.
///
/// Any value passed into the sink (via the `futures::Sink` trait) will be converted into an
/// immediate `ImmediateFrame` and sent.
pub struct ImmediateSink<A, S> {
    /// The underlying sink where items are written.
    sink: S,
    /// Phantom data for the immediate array type.
    _phantom: PhantomData<A>,
}

impl<A, S> ImmediateSink<A, S> {
    /// Creates a new immediate sink on top of the given stream.
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            _phantom: PhantomData,
        }
    }
}

impl<A, S, T> Sink<T> for ImmediateSink<A, S>
where
    A: Unpin,
    ImmediateFrame<A>: From<T>,
    S: Sink<ImmediateFrame<A>> + Unpin,
{
    type Error = <S as Sink<ImmediateFrame<A>>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_ready_unpin(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let immediate = item.into();
        self.get_mut().sink.start_send_unpin(immediate)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_flush_unpin(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_close_unpin(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::{FutureExt, SinkExt};

    use crate::{fixed_size::ImmediateSink, tests::TestingSink};

    #[test]
    fn simple_sending() {
        let output = Arc::new(TestingSink::new());
        let mut sink = ImmediateSink::new(output.clone().into_ref());

        sink.send(0x1234u32).now_or_never().unwrap().unwrap();
        assert_eq!(output.get_contents(), &[0x34, 0x12, 0x00, 0x00]);

        sink.send(0xFFFFFFFFu32).now_or_never().unwrap().unwrap();
        assert_eq!(
            output.get_contents(),
            &[0x34, 0x12, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF]
        );

        sink.send(0x78563412u32).now_or_never().unwrap().unwrap();
        assert_eq!(
            output.get_contents(),
            &[0x34, 0x12, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x12, 0x34, 0x56, 0x78]
        );
    }
}
