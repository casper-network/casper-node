//! Immediate (small/fixed size) item sink and stream.
//!
//! `ImmediateSink` allows sending items for which `Into<ImmediateSink<_>>` is
//! implemented. Typically this is true for small atomic types like `u32`, which are encoded as
//! little endian in throughout this crate.
//!
//! No additional headers are added, as immediate values are expected to be of fixed size.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{ready, Sink, SinkExt, Stream, StreamExt};
use thiserror::Error;

use crate::{FromFixedSize, ImmediateFrame};

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

/// Stream of immediate values.
///
/// Reconstructs immediates from variably sized frames. The incoming frames are assumed to be all of
/// the same size.
pub struct ImmediateStream<S, T> {
    stream: S,
    _type: PhantomData<T>,
}

/// Error occurring during immediate stream reading.
#[derive(Debug, Error)]
pub enum ImmediateStreamError {
    /// The incoming frame was of the wrong size.
    #[error("wrong size for immediate frame, expected {expected}, got {actual}")]
    WrongSize { actual: usize, expected: usize },
}

impl<T, S> ImmediateSink<T, S> {
    /// Creates a new immediate sink on top of the given stream.
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            _phantom: PhantomData,
        }
    }
}

impl<S, T> ImmediateStream<S, T> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            _type: PhantomData,
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

impl<S, T> Stream for ImmediateStream<S, T>
where
    T: FromFixedSize + Unpin,
    S: Stream<Item = Bytes> + Unpin,
{
    type Item = Result<T, ImmediateStreamError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();

        match ready!(self_mut.stream.poll_next_unpin(cx)) {
            Some(frame) => {
                let slice: &[u8] = &frame;

                Poll::Ready(Some(T::from_slice(slice).ok_or({
                    ImmediateStreamError::WrongSize {
                        actual: slice.len(),
                        expected: T::WIRE_SIZE,
                    }
                })))
            }
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use futures::{stream, FutureExt, SinkExt};

    use crate::{
        fixed_size::ImmediateSink,
        tests::{collect_stream_results, TestingSink},
    };

    use super::ImmediateStream;

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

    #[test]
    fn simple_stream() {
        let input = vec![
            Bytes::copy_from_slice(&[0x78, 0x56, 0x34, 0x12]),
            Bytes::copy_from_slice(&[0xDD, 0xCC, 0xBB, 0xAA]),
        ];

        let stream = ImmediateStream::<_, u32>::new(stream::iter(input));

        assert_eq!(collect_stream_results(stream), &[0x12345678, 0xAABBCCDD]);
    }
}
