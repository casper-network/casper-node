use std::{borrow::BorrowMut, pin::Pin, task::Context};

use bytes::{Buf, Bytes};
use futures::Future;

use crate::{FrameSink, FrameSinkError, ImmediateFrame};

// use std::marker::PhantomData;

// use bytes::{Buf, Bytes};

// use crate::{FrameSink, GenericBufSender};

// #[derive(Debug)]
// pub struct Chunker<S, F> {
//     frame_sink: S,
//     _frame_phantom: PhantomData<F>,
// }

type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// TODO: Turn into non-anonymous future with zero allocations.
fn x<B, S>(
    mut frame: B,
    chunk_size: usize,
    mut sink: S,
) -> impl Future<Output = Result<(), FrameSinkError>>
where
    B: Buf,
    for<'a> &'a mut S: FrameSink<SingleChunk>,
{
    let num_frames = (frame.remaining() + chunk_size - 1) / chunk_size;

    let chunk_id_ceil: u8 = num_frames.try_into().unwrap(); // TODO: Report error.
    async move {
        for n in 0..num_frames {
            let chunk_id = if n == 0 {
                chunk_id_ceil
            } else {
                // Will never overflow, since `chunk_id_ceil` already fits into a `u8`.
                n as u8
            };

            // Note: If the given frame is `Bytes`, `copy_to_bytes` should be a cheap copy.
            let chunk_data = frame.copy_to_bytes(chunk_size);
            let chunk = ImmediateFrame::from(chunk_id).chain(chunk_data);

            // We have produced a chunk, now send it.
            sink.send_frame(chunk).await?;
        }

        Result::<(), FrameSinkError>::Ok(())
    }
}

// NEW
// struct ChunkSender<F, S> {
//     sent: usize,
//     chunk_size: usize,
//     frame: F,
//     sink: S,
// }

// impl<F, S> Future for ChunkSender<F, S> {
//     type Output = Result<(), FrameSinkError>;

//     fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {

//     }
// }
// END NEW

// // TODO: Use special single-byte prefix type.
// type SingleChunk<F = bytes::buf::Chain<Bytes, F>;
// struct SingleChunk {

// }

// impl<'a, S, F> FrameSink<F> for &'a mut Chunker<S, F>
// where
//     F: Buf + Send,
// {
//     type SendFrameFut = GenericBufSender<'a, ChunkedFrames<F>, W>;

//     fn send_frame(self, frame: F) -> Self::SendFrameFut {
//         todo!()
//         // let length = frame.remaining() as u64; // TODO: Try into + handle error.
//         // let length_prefixed_frame = Bytes::copy_from_slice(&length.to_le_bytes()).chain(frame);
//         // GenericBufSender::new(length_prefixed_frame, &mut self.writer)
//     }
// }
