//! Frame reading and writing
//!
//! Frame readers and writers are responsible for writing a [`Bytes`] frame to a an `AsyncWrite`, or
//! reading them from `AsyncRead`. They can be given a flexible function to encode and decode
//! frames.

mod length_delimited;

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use futures::{AsyncRead, Stream};
use thiserror::Error;

/// Frame decoder.
///
/// A frame decoder is responsible for extracting a frame from a reader's internal buffer.
pub trait Decoder {
    /// Decoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Decodes a frame from a buffer.
    ///
    /// If `buffer` contains enough
    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Self::Error>;
}

/// Frame encoder.
///
/// A frame encoder adds the framing envelope (or replaces the frame entirely) of a given raw frame.
pub trait Encoder {
    /// Encoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The wrapped frame resulting from encoding the given raw frame.
    ///
    /// While this can be simply `Bytes`, using something like `bytes::Chain` allows for more
    /// efficient encoding here.
    type WrappedFrame: Buf + Send + Sync + 'static;

    /// Encode a frame.
    ///
    /// The resulting `Bytes` should be the bytes to send into the outgoing stream, it must contain
    /// the information required for an accompanying `Decoder` to be able to reconstruct the frame
    /// from a raw byte stream.
    fn encode_frame(&mut self, raw_frame: Bytes) -> Result<Self::WrappedFrame, Self::Error>;
}

/// The outcome of a [`decode_frame`] call.
#[derive(Debug, Error)]
pub enum DecodeResult<E> {
    /// A complete frame was decoded.
    Frame(BytesMut),
    /// No frame could be decoded, an unknown amount of bytes is still required.
    Incomplete,
    /// No frame could be decoded, but the remaining amount of bytes required is known.
    Remaining(usize),
    /// Irrecoverably failed to decode frame.
    Failed(E),
}

/// Frame reader for frames.
pub struct FrameReader<D, R> {
    /// The decoder used to decode frames.
    decoder: D,
    /// The underlying async bytestream being read.
    stream: R,
    /// Internal buffer for incomplete frames.
    buffer: BytesMut,
    /// Maximum number of bytes to read.
    max_read_buffer_increment: usize,
}

impl<D, R> FrameReader<D, R> {
    /// Creates a new frame reader on a given stream with the given read buffer increment.
    pub fn new(decoder: D, stream: R, max_read_buffer_increment: usize) -> Self {
        Self {
            decoder,
            stream,
            buffer: BytesMut::new(),
            max_read_buffer_increment,
        }
    }

    /// Deconstructs a frame reader into decoder, reader and buffer.
    pub fn into_parts(self) -> (D, R, BytesMut) {
        (self.decoder, self.stream, self.buffer)
    }
}

impl<D, R> Stream for FrameReader<D, R>
where
    D: Decoder + Unpin,
    R: AsyncRead + Unpin,
{
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let FrameReader {
            ref mut stream,
            ref mut decoder,
            ref mut buffer,
            max_read_buffer_increment,
        } = self.get_mut();
        loop {
            let next_read = match decoder.decode_frame(buffer) {
                DecodeResult::Frame(frame) => return Poll::Ready(Some(Ok(frame.freeze()))),
                DecodeResult::Incomplete => *max_read_buffer_increment,
                DecodeResult::Remaining(remaining) => remaining.min(*max_read_buffer_increment),
                DecodeResult::Failed(error) => {
                    return Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, error))))
                }
            };

            let start = buffer.len();
            let end = start + next_read;
            buffer.resize(end, 0x00);

            match Pin::new(&mut *stream).poll_read(cx, &mut buffer[start..end]) {
                Poll::Ready(Ok(bytes_read)) => {
                    buffer.truncate(start + bytes_read);
                    if bytes_read == 0 {
                        return Poll::Ready(None);
                    }
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
