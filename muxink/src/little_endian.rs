/// Little-endian integer codec.
use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, SinkExt, Stream, StreamExt};
use thiserror::Error;

/// Little endian integer codec.
///
/// Integers encoded or decoded through this sink/stream wrapper are encoded/decoded as little
/// endian integers (via `ImmediateFrame` when encoding) before being forwarded to the underlying
/// sink/stream.
///
/// This data structure implements either `Stream` or `Sink`, depending on the wrapped `S`.
#[derive(Debug)]
pub struct LittleEndian<T, S> {
    inner: S,
    /// Phantom data pinning the accepted type.
    ///
    /// While an encoder would not need to restrict `T`, it still is limited to a single type for
    /// type safety.
    _type_pin: PhantomData<T>,
}

impl<T, S> LittleEndian<T, S> {
    /// Creates a new little endian sink/stream.
    pub fn new(inner: S) -> Self {
        LittleEndian {
            inner,
            _type_pin: PhantomData,
        }
    }

    /// Returns the wrapped stream.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

/// Decoding error for little endian decoding stream.
#[derive(Debug, Error)]
pub enum DecodeError<E>
where
    E: std::error::Error,
{
    /// The incoming `Bytes` object was of the wrong size.
    #[error("Size mismatch, expected {expected} bytes, got {actual}")]
    SizeMismatch { expected: usize, actual: usize },
    /// The wrapped stream returned an error.
    #[error(transparent)]
    Stream(#[from] E),
}

macro_rules! int_codec {
    ($ty:ty) => {
        impl<S> Sink<$ty> for LittleEndian<$ty, S>
        where
            S: Sink<crate::ImmediateFrame<[u8; (<$ty>::BITS / 8) as usize]>> + Unpin,
        {
            type Error =
                <S as Sink<crate::ImmediateFrame<[u8; (<$ty>::BITS / 8) as usize]>>>::Error;

            #[inline]
            fn poll_ready(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Result<(), Self::Error>> {
                self.as_mut().inner.poll_ready_unpin(cx)
            }

            #[inline]
            fn start_send(mut self: Pin<&mut Self>, item: $ty) -> Result<(), Self::Error> {
                let frame = crate::ImmediateFrame::<[u8; (<$ty>::BITS / 8) as usize]>::from(item);
                self.as_mut().inner.start_send_unpin(frame)
            }

            #[inline]
            fn poll_flush(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Result<(), Self::Error>> {
                self.as_mut().inner.poll_flush_unpin(cx)
            }

            #[inline]
            fn poll_close(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Result<(), Self::Error>> {
                self.as_mut().inner.poll_close_unpin(cx)
            }
        }

        impl<S, E> Stream for LittleEndian<$ty, S>
        where
            S: Stream<Item = Result<Bytes, E>> + Unpin,
            E: std::error::Error,
        {
            type Item = Result<$ty, DecodeError<E>>;

            fn poll_next(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                let raw_result = futures::ready!(self.as_mut().inner.poll_next_unpin(cx));

                let raw_item = match raw_result {
                    None => return Poll::Ready(None),
                    Some(Err(e)) => return Poll::Ready(Some(Err(DecodeError::Stream(e)))),
                    Some(Ok(v)) => v,
                };

                let bytes_le: [u8; (<$ty>::BITS / 8) as usize] = match (&*raw_item).try_into() {
                    Ok(v) => v,
                    Err(_) => {
                        return Poll::Ready(Some(Err(DecodeError::SizeMismatch {
                            expected: (<$ty>::BITS / 8) as usize,
                            actual: raw_item.len(),
                        })))
                    }
                };
                Poll::Ready(Some(Ok(<$ty>::from_le_bytes(bytes_le))))
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                self.inner.size_hint()
            }
        }
    };
}

// Implement for known integer types.
int_codec!(u16);
int_codec!(u32);
int_codec!(u64);
int_codec!(u128);
int_codec!(i16);
int_codec!(i32);
int_codec!(i64);
int_codec!(i128);

#[cfg(test)]
mod tests {
    use futures::{io::Cursor, FutureExt, SinkExt};

    use crate::{
        framing::fixed_size::FixedSize,
        io::{FrameReader, FrameWriter},
        testing::collect_stream_results,
        ImmediateFrameU32,
    };

    use super::LittleEndian;

    /// Decodes the input string, returning the decoded frames and the remainder.
    fn run_decoding_stream(input: &[u8], chomp_size: usize) -> (Vec<u32>, Vec<u8>) {
        let stream = Cursor::new(input);

        let mut reader =
            LittleEndian::<u32, _>::new(FrameReader::new(FixedSize::new(4), stream, chomp_size));

        let decoded: Vec<u32> = collect_stream_results(&mut reader);

        // Extract the remaining data.
        let (_decoder, cursor, buffer) = reader.into_inner().into_parts();
        let mut remaining = Vec::new();
        remaining.extend(buffer.into_iter());
        let cursor_pos = cursor.position() as usize;
        remaining.extend(&cursor.into_inner()[cursor_pos..]);

        (decoded, remaining)
    }

    #[test]
    fn simple_stream_decoding_works() {
        for chomp_size in 1..=1024 {
            let input = b"\x01\x02\x03\x04\xAA\xBB\xCC\xDD";
            let (decoded, remainder) = run_decoding_stream(input, chomp_size);
            assert_eq!(decoded, &[0x04030201, 0xDDCCBBAA]);
            assert!(remainder.is_empty());
        }
    }

    #[test]
    fn empty_stream_is_empty() {
        let input = b"";

        let (decoded, remainder) = run_decoding_stream(input, 3);
        assert!(decoded.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn encodes_simple_cases_correctly() {
        let seq = [0x01020304u32, 0xAABBCCDD];
        let outcomes: &[&[u8]] = &[b"\x04\x03\x02\x01", b"\xDD\xCC\xBB\xAA"];

        for (input, &expected) in seq.into_iter().zip(outcomes.iter()) {
            let mut output: Vec<u8> = Vec::new();
            let mut writer = LittleEndian::<u32, _>::new(
                FrameWriter::<ImmediateFrameU32, _, _>::new(FixedSize::new(4), &mut output),
            );
            writer
                .send(input)
                .now_or_never()
                .expect("send did not finish")
                .expect("sending should not fail");
            assert_eq!(&output, expected);
        }
    }
}
