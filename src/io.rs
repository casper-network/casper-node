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
use futures::{ready, AsyncRead, AsyncWrite, Sink, Stream};
use thiserror::Error;

use crate::try_ready;

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
pub trait Encoder<F> {
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
    fn encode_frame(&mut self, raw_frame: F) -> Result<Self::WrappedFrame, Self::Error>;
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

/// Reader for frames being encoded.
pub struct FrameReader<D, R> {
    /// Decoder used to decode frames.
    decoder: D,
    /// Underlying async bytestream being read.
    stream: R,
    /// Internal buffer for incomplete frames.
    buffer: BytesMut,
    /// Maximum number of bytes to read.
    max_read_buffer_increment: usize,
}

/// Writer for frames.
pub struct FrameWriter<F, E: Encoder<F>, W> {
    /// The encoder used to encode outgoing frames.
    encoder: E,
    /// Underlying async bytestream being written.
    stream: W,
    /// The frame in process of being sent.
    current_frame: Option<E::WrappedFrame>,
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

impl<F, E, W> FrameWriter<F, E, W>
where
    E: Encoder<F>,
{
    /// Creates a new frame writer with the given encoder.
    pub fn new(encoder: E, stream: W) -> Self {
        Self {
            encoder,
            stream,
            current_frame: None,
        }
    }

    pub fn finish_sending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>
    where
        Self: Sink<F> + Unpin,
        F: Buf,
        W: AsyncWrite + Unpin,
    {
        loop {
            match self.current_frame {
                // No more frame to send, we're ready.
                None => return Poll::Ready(Ok(())),

                Some(ref mut current_frame) => {
                    // TODO: Implement support for `poll_write_vectored`.

                    let wpin = Pin::new(&mut self.stream);
                    match wpin.poll_write(cx, current_frame.chunk()) {
                        Poll::Ready(Ok(bytes_written)) => {
                            current_frame.advance(bytes_written);

                            // If we're done, clear the current frame and return.
                            if !current_frame.has_remaining() {
                                self.current_frame.take();
                                return Poll::Ready(Ok(()));
                            }

                            // Otherwise, repeat the loop.
                        }
                        // Error occured, we have to abort.
                        Poll::Ready(Err(error)) => {
                            return Poll::Ready(Err(error));
                        }
                        // The underlying output stream is blocked, no progress can be made.
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }
}

impl<F, E, W> Sink<F> for FrameWriter<F, E, W>
where
    Self: Unpin,
    E: Encoder<F>,
    F: Buf,
    W: AsyncWrite + Unpin,
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        try_ready!(ready!(self_mut.finish_sending(cx)));

        // Even though there may be outstanding writes on the underlying stream, our item buffer is
        // empty, so we are ready to accept the next item.
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let wrapped_frame = self
            .encoder
            .encode_frame(item)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.current_frame = Some(wrapped_frame);

        // We could eagler poll and send to the underlying writer here, but for ease of
        // implementation we don't.

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        // We need to make sure all data is buffered to the underlying stream first.
        try_ready!(ready!(self_mut.finish_sending(cx)));

        // Finally it makes sense to flush.
        let wpin = Pin::new(&mut self_mut.stream);
        wpin.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        // Finish buffering our outstanding item.
        try_ready!(ready!(self_mut.finish_sending(cx)));

        let wpin = Pin::new(&mut self_mut.stream);
        wpin.poll_close(cx)
    }
}
