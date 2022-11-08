//! Frame reading and writing
//!
//! [`FrameReader`]s and [`FrameWriter`]s are responsible for writing a [`bytes::Bytes`] frame to an
//! [`AsyncWrite`] writer, or reading them from [`AsyncRead`] reader. While writing works for any
//! value that implements the [`bytes::Buf`] trait, decoding requires an implementation of the
//! [`FrameDecoder`] trait.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use futures::{ready, AsyncRead, AsyncWrite, Sink, Stream};

use crate::{
    framing::{DecodeResult, FrameDecoder, FrameEncoder},
    try_ready,
};

/// Reads frames from an underlying reader.
///
/// Uses the given [`FrameDecoder`] `D` to read frames from the underlying IO.
#[derive(Debug)]
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
///
/// Writes a frame to the underlying writer after encoding it using the given [`FrameEncoder`].
///
/// # Cancellation safety
///
/// The [`Sink`] methods on [`FrameWriter`] are cancellation safe. Only a single item is buffered
/// inside the writer itself.
#[derive(Debug)]
pub struct FrameWriter<F, E, W>
where
    E: FrameEncoder<F>,
    F: Buf,
{
    /// The encoder used to encode outgoing frames.
    encoder: E,
    /// Underlying async bytestream being written.
    stream: W,
    /// The frame in process of being sent.
    current_frame: Option<E::Output>,
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
    D: FrameDecoder + Unpin,
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
                DecodeResult::Item(frame) => return Poll::Ready(Some(Ok(frame))),
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
                Poll::Pending => {
                    buffer.truncate(start);
                    return Poll::Pending;
                }
            }
        }
    }
}

impl<F, E, W> FrameWriter<F, E, W>
where
    E: FrameEncoder<F>,
    <E as FrameEncoder<F>>::Output: Buf,
    F: Buf,
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
        W: AsyncWrite + Unpin,
    {
        loop {
            match self.current_frame {
                // No more frame to send, we're ready.
                None => return Poll::Ready(Ok(())),

                Some(ref mut current_frame) => {
                    // TODO: Implement support for `poll_write_vectored`.

                    let stream_pin = Pin::new(&mut self.stream);
                    match stream_pin.poll_write(cx, current_frame.chunk()) {
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
    E: FrameEncoder<F>,
    <E as FrameEncoder<F>>::Output: Buf,
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

        // We could eaglerly poll and send to the underlying writer here, but for ease of
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

#[cfg(test)]
mod tests {
    use std::{mem, pin::Pin};

    use bytes::Bytes;
    use futures::{
        io::Cursor, sink::SinkExt, stream::StreamExt, AsyncRead, AsyncReadExt, AsyncWriteExt,
        FutureExt,
    };
    use tokio::io::DuplexStream;
    use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

    use super::{FrameReader, FrameWriter};
    use crate::framing::length_delimited::LengthDelimited;

    /// Async reader used by a test below to gather all underlying
    /// read calls and their results.
    struct AsyncReadCounter<S> {
        stream: S,
        reads: Vec<usize>,
    }

    impl<S: AsyncRead + Unpin> AsyncReadCounter<S> {
        pub fn new(stream: S) -> Self {
            Self {
                stream,
                reads: vec![],
            }
        }

        pub fn reads(&self) -> &[usize] {
            &self.reads
        }
    }

    impl<S: AsyncRead + Unpin> AsyncRead for AsyncReadCounter<S> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let read_result = Pin::new(&mut self.stream).poll_read(cx, buf);
            if let std::task::Poll::Ready(Ok(len)) = read_result {
                self.reads.push(len);
            }
            read_result
        }
    }

    /// A basic integration test for sending data across an actual TCP stream.
    #[tokio::test]
    async fn simple_tcp_send_recv() {
        let server = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("could not bind");
        let server_addr = server.local_addr().expect("no local addr");
        let frame_to_send = b"asdf12345asdf";

        let server_handle = tokio::spawn(async move {
            let (incoming, _client_peer_addr) = server
                .accept()
                .await
                .expect("could not accept connection on server side");

            let mut frame_reader = FrameReader::new(LengthDelimited, incoming.compat(), 32);
            let outcome = frame_reader
                .next()
                .await
                .expect("closed unexpectedly")
                .expect("receive failed");

            assert_eq!(&outcome.to_vec(), frame_to_send);
        });

        let client = tokio::net::TcpStream::connect(server_addr)
            .await
            .expect("failed to connect");
        let mut frame_writer = FrameWriter::new(LengthDelimited, client.compat());
        frame_writer
            .send(Bytes::from(&frame_to_send[..]))
            .await
            .expect("could not sendn data");

        server_handle.await.expect("joining failed");
    }

    #[test]
    fn frame_reader_reads_without_consuming_extra_bytes() {
        const FRAME: &[u8; 16] = b"abcdef0123456789";
        const COPIED_FRAME_LEN: u16 = 8;
        let mut encoded_longer_frame = COPIED_FRAME_LEN.to_le_bytes().to_vec();
        encoded_longer_frame.extend_from_slice(FRAME.as_slice());

        let cursor = Cursor::new(encoded_longer_frame.as_slice());
        let mut reader = FrameReader::new(LengthDelimited, cursor, 1000);

        let first_frame = reader.next().now_or_never().unwrap().unwrap().unwrap();
        assert_eq!(&first_frame, &FRAME[..COPIED_FRAME_LEN as usize]);

        let (_, mut cursor, mut buffer) = reader.into_parts();
        let mut unread_cursor_buf = vec![];
        let unread_cursor_len = cursor
            .read_to_end(&mut unread_cursor_buf)
            .now_or_never()
            .unwrap()
            .unwrap();
        buffer.extend_from_slice(&unread_cursor_buf[..unread_cursor_len]);
        assert_eq!(&buffer, &FRAME[COPIED_FRAME_LEN as usize..]);
    }

    #[test]
    fn frame_reader_does_not_allow_exceeding_maximum_size() {
        const FRAME: &[u8; 16] = b"abcdef0123456789";
        const COPIED_FRAME_LEN: u16 = 16;
        const MAX_READ_BUF_INCREMENT: usize = 5;
        let mut encoded_longer_frame = COPIED_FRAME_LEN.to_le_bytes().to_vec();
        encoded_longer_frame.extend_from_slice(FRAME.as_slice());

        let cursor = AsyncReadCounter::new(Cursor::new(encoded_longer_frame.as_slice()));
        let mut reader = FrameReader::new(LengthDelimited, cursor, MAX_READ_BUF_INCREMENT);

        let first_frame = reader.next().now_or_never().unwrap().unwrap().unwrap();
        assert_eq!(&first_frame, &FRAME[..COPIED_FRAME_LEN as usize]);

        let (_, counter, _) = reader.into_parts();
        // Considering we have a `max_read_buffer_increment` of 5, the encoded length
        // is a `u16`, `sizeof(u16)` is 2, and the length of the original frame is 16,
        // reads should be:
        // [2 + (5 - 2), 5, 5, 5 - 2]
        assert_eq!(
            counter.reads(),
            [
                MAX_READ_BUF_INCREMENT,
                MAX_READ_BUF_INCREMENT,
                MAX_READ_BUF_INCREMENT,
                MAX_READ_BUF_INCREMENT - mem::size_of::<u16>()
            ]
        );
    }

    #[tokio::test]
    async fn frame_reader_handles_0_sized_read() {
        const FRAME: &[u8; 16] = b"abcdef0123456789";
        const COPIED_FRAME_LEN: u16 = 16;
        const MAX_READ_BUF_INCREMENT: usize = 6;
        let mut encoded_longer_frame = COPIED_FRAME_LEN.to_le_bytes().to_vec();
        encoded_longer_frame.extend_from_slice(FRAME.as_slice());

        let (sender, receiver) = tokio::io::duplex(1000);
        let mut reader = FrameReader::new(
            LengthDelimited,
            receiver.compat(),
            (COPIED_FRAME_LEN >> 1).into(),
        );

        // We drop the sender at the end of the async block in order to simulate
        // a 0-sized read.
        let send_fut = async move {
            sender
                .compat()
                .write_all(&encoded_longer_frame[..MAX_READ_BUF_INCREMENT])
                .await
                .unwrap();
        };
        let recv_fut = async { reader.next().await };
        let (_, received) = tokio::join!(send_fut, recv_fut);
        assert!(received.is_none());
    }

    #[tokio::test]
    async fn frame_reader_handles_early_eof() {
        const FRAME: &[u8; 16] = b"abcdef0123456789";
        const COPIED_FRAME_LEN: u16 = 16;
        let mut encoded_longer_frame = (COPIED_FRAME_LEN + 1).to_le_bytes().to_vec();
        encoded_longer_frame.extend_from_slice(FRAME.as_slice());

        let cursor = Cursor::new(encoded_longer_frame.as_slice());
        let mut reader = FrameReader::new(LengthDelimited, cursor, 1000);

        assert!(reader.next().await.is_none());
    }

    #[test]
    fn frame_writer_writes_frames_correctly() {
        const FIRST_FRAME: &[u8; 16] = b"abcdef0123456789";
        const SECOND_FRAME: &[u8; 9] = b"dead_beef";

        let mut frame_writer: FrameWriter<Bytes, LengthDelimited, Vec<u8>> =
            FrameWriter::new(LengthDelimited, Vec::new());
        frame_writer
            .send((&FIRST_FRAME[..]).into())
            .now_or_never()
            .unwrap()
            .unwrap();
        let FrameWriter {
            encoder: _,
            stream,
            current_frame: _,
        } = &frame_writer;
        let mut encoded_longer_frame = (FIRST_FRAME.len() as u16).to_le_bytes().to_vec();
        encoded_longer_frame.extend_from_slice(FIRST_FRAME.as_slice());
        assert_eq!(stream.as_slice(), encoded_longer_frame);

        frame_writer
            .send((&SECOND_FRAME[..]).into())
            .now_or_never()
            .unwrap()
            .unwrap();
        let FrameWriter {
            encoder: _,
            stream,
            current_frame: _,
        } = &frame_writer;
        encoded_longer_frame
            .extend_from_slice((SECOND_FRAME.len() as u16).to_le_bytes().as_slice());
        encoded_longer_frame.extend_from_slice(SECOND_FRAME.as_slice());
        assert_eq!(stream.as_slice(), encoded_longer_frame);
    }

    #[tokio::test]
    async fn frame_writer_handles_0_size() {
        const FRAME: &[u8; 16] = b"abcdef0123456789";

        let (sender, receiver) = tokio::io::duplex(1000);
        let mut frame_writer: FrameWriter<Bytes, LengthDelimited, Compat<DuplexStream>> =
            FrameWriter::new(LengthDelimited, sender.compat());
        // Send a first frame.
        frame_writer.send((&FRAME[..]).into()).await.unwrap();

        // Send an empty frame.
        // We drop the sender at the end of the async block to mark the end of
        // the stream.
        let send_fut = async move { frame_writer.send(Bytes::new()).await.unwrap() };

        let recv_fut = async {
            let mut buf = Vec::new();
            receiver.compat().read_to_end(&mut buf).await.unwrap();
            buf
        };

        let (_, received) = tokio::join!(send_fut, recv_fut);
        assert_eq!(
            &received[FRAME.len() + mem::size_of::<u16>()..],
            0u16.to_le_bytes()
        );
    }
}
