use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt};
use pin_project::pin_project;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_util::codec::{BytesCodec, FramedWrite};

// Idea for multiplexer:

trait Channel {
    fn into_u8(self) -> u8;
    fn inc(self) -> Self;
}

// For multiplexer, simply track which is the active channel, then if not active channel, return not
// ready. How to track who wants to send something? Do we need to learn how waker's work?

// Quick-and-dirty: Use streams/FuturesUnorded or some other sort of polling mechanism (i.e. create
// a Stream/Sink pair for every `Channel`, allow taking these?).

// Not having extra handles, simply ingest `(chan_id, msg)` -- does shift the burden of parallizing
// onto the caller. We can figure out some way of setting a bool (or a waker?) for the active
// channel (and registering interest), then waiting until there are no more "active" channels
// between the current pointer and us. We get our slot, send and continue.
//
// The actual Sink would take tuples in this case. Still would need to guard access to it, so maybe
// not a good fit. Note, we do not expect backpressure to matter here!

// Alternative: No fair scheduling, simply send ASAP, decorated with multiplexer ID.
// What happens if two unlimited streams are blasting at max speed? Starvation.

// Synchronization primitive: Round-robin number/ticket generator?

// Potentially better idea:
trait SinkTransformer {
    type Input;
    type Output;
    type Error;

    fn push_item(&mut self, item: Self::Input) -> Result<(), Self::Error>;

    fn next_item(&mut self) -> Result<Option<Self::Output>, Self::Error>;
}

struct FrameSink<S, T> {
    sink: S,
    transformer: T,
}

#[derive(Debug, Error)]
enum FrameSinkError<I, S, T>
where
    S: Sink<I>,
    T: SinkTransformer<Input = I>,
{
    #[error("sink failed")]
    SinkFailed(#[source] <S as Sink<I>>::Error),
    #[error("transformer failed")]
    TransformerFailed(#[source] <T as SinkTransformer>::Error),
}

impl<S, T> Sink<T::Input> for FrameSink<S, T>
where
    T: SinkTransformer<Input = >,
{
    type Error = FrameSinkError<T::Input, S, T>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn start_send(self: Pin<&mut Self>, item: T::Input) -> Result<(), Self::Error> {
        todo!()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}

// CHUNKER

const CHUNKER_CHUNK_SIZE: usize = 4096;

#[pin_project]
struct Chunker<S> {
    chunk_size: usize,
    data_buffer: Option<Bytes>,
    #[pin]
    sink: S,
    next_chunk: u8,
    chunk_count: u8,
}

impl<S> Chunker<S> {
    fn new(sink: S, chunk_size: usize) -> Self {
        todo!()
        // Chunker {
        //     sink,
        //     data_buffer: None,
        //     bytes_sent: 0,
        //     header_sent: false,
        //     chunk_size,
        // }
    }
}

impl<S> Chunker<S> {
    fn make_progress_sending_chunks(&mut self) {}
}

impl<S> Sink<Bytes> for Chunker<S>
where
    S: Sink<Bytes>,
{
    type Error = <S as Sink<Bytes>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.data_buffer.is_none() {
            let this = self.project();

            // Report ready only when our data buffer is empty and we're ready to store the next
            // header in the underlying sink.
            this.sink.poll_ready(cx)
        } else {
            Poll::Pending
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        let chunk_count = item.len() + self.chunk_size - 1 / self.chunk_size;

        // TODO: Check if size exceeds maximum size.

        self.chunk_count = chunk_count as u8;
        self.next_chunk = 0;
        self.data_buffer = Some(item);

        // TODO: Use statically allocated BytesMut to avoid heap allocations.
        let header = Bytes::copy_from_slice(&[self.chunk_count]);

        // Move header into the underlying sink.
        let this = self.project();
        this.sink.start_send(header)?;

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // TODO: Ensure zero-sized data is handled correctly.

        match self.data_buffer {
            Some(data_buffer) => {
                // We know we got more data to send, so ensure the underlying sink is ready.
                {
                    let this = self.project();
                    match this.sink.poll_ready(cx) {
                        Poll::Ready(Ok(())) => {
                            // Alright, let's go!
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(e));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }

                let chunk_start = self.next_chunk as usize * self.chunk_size;
                let chunk_end =
                    ((self.next_chunk as usize + 1) * self.chunk_size).min(data_buffer.len());
                let chunk = data_buffer.slice(chunk_start..chunk_end);

                {
                    let this = self.project();
                    if let Err(err) = this.sink.start_send(chunk) {
                        return Poll::Ready(Err(err));
                    }
                }

                if self.next_chunk == self.chunk_count {
                    // We are all done sending chunks, release data buffer to indicate we're done.
                    self.data_buffer = None;
                } else {
                    self.next_chunk += 1;
                }

                // We need to run this in a loop, since calling `poll_flush` is the next step.
                todo!()
            }
            None => {
                // We sent all we can send, but we may need to flush the underlying sink.
                let this = self.project();
                this.sink.poll_flush(cx)
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}

struct Dechunker {
    stream: T,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("localhost:12345").await?;

    let mut codec = FramedWrite::new(stream, BytesCodec::new());
    codec.send(BytesMut::from(&b"xxx\n"[..])).await?;

    Ok(())
}
