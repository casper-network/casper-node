// use std::{
//     pin::Pin,
//     task::{Context, Poll},
// };

// use bytes::{Buf, Bytes};
// use futures::{future::BoxFuture, Future, FutureExt};
// use pin_project::pin_project;

// use crate::{FrameSink, FrameSinkError, ImmediateFrame};

// // use std::marker::PhantomData;

// // use bytes::{Buf, Bytes};

// // use crate::{FrameSink, GenericBufSender};

// // #[derive(Debug)]
// // pub struct Chunker<S, F> {
// //     frame_sink: S,
// //     _frame_phantom: PhantomData<F>,
// // }

// trait Foo {
//     type Fut: Future<Output = usize>;

//     fn mk_fut(self) -> Self::Fut;
// }

// type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

// /// TODO: Turn into non-anonymous future with zero allocations.
// async fn x<B, S>(frame: B, chunk_size: usize, mut sink: S) -> Result<(), FrameSinkError>
// where
//     B: Buf,
//     for<'a> &'a mut S: FrameSink<SingleChunk>,
// {
//     for chunk in chunk_frame(frame, chunk_size) {
//         sink.send_frame(chunk).await?;
//     }
//     Ok(())
// }

// /// Chunks a frame into ready-to-send chunks.
// ///
// /// # Notes
// ///
// /// Internally, data is copied into chunks by using `Buf::copy_to_bytes`. It is advisable to use a
// /// `B` that has an efficient implementation for this that avoids copies, like `Bytes` itself.
// fn chunk_frame<B: Buf>(mut frame: B, chunk_size: usize) -> impl Iterator<Item = SingleChunk> {
//     let num_frames = (frame.remaining() + chunk_size - 1) / chunk_size;

//     let chunk_id_ceil: u8 = num_frames.try_into().unwrap();

//     (0..num_frames).into_iter().map(move |n| {
//         let chunk_id = if n == 0 {
//             chunk_id_ceil
//         } else {
//             // Will never overflow, since `chunk_id_ceil` already fits into a `u8`.
//             n as u8
//         };

//         let chunk_data = frame.copy_to_bytes(chunk_size);
//         ImmediateFrame::from(chunk_id).chain(chunk_data)
//     })
// }

// #[pin_project]
// struct ChunkSender<S> {
//     chunks: Box<dyn Iterator<Item = SingleChunk>>,
//     chunk_in_progress: Option<Box<dyn Future<Output = Result<S, FrameSinkError>> + Send>>,
//     sink: Option<S>,
// }

// impl<S> Future for ChunkSender<S>
// where
//     S: FrameSink<SingleChunk>,
// {
//     type Output = Result<(), FrameSinkError>;

//     fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
//         match self.chunks.next() {
//             Some(current_chunk) => {
//                 let sink = self.sink.take().unwrap(); // TODO

//                 let mut fut: Pin<
//                     Box<dyn Future<Output = Result<(), FrameSinkError>> + Send + Unpin>,
//                 > = sink.send_frame(current_chunk).boxed();

//                 // TODO: Simplify?
//                 let mut pinned_fut = Pin::new(&mut fut);
//                 match pinned_fut.poll(cx) {
//                     Poll::Ready(_) => {
//                         todo!()
//                     }
//                     Poll::Pending => {
//                         // Store the future for future polling.
//                         self.chunk_in_progress = Some(Pin::into_inner(fut));

//                         // We need to wait to make progress.
//                         Poll::Pending
//                     }
//                 }
//             }
//             None => {
//                 // We're all done sending.
//                 Poll::Ready(Ok(()))
//             }
//         }
//     }
// }
// // END NEW

// // // TODO: Use special single-byte prefix type.
// // type SingleChunk<F = bytes::buf::Chain<Bytes, F>;
// // struct SingleChunk {

// // }

// // impl<'a, S, F> FrameSink<F> for &'a mut Chunker<S, F>
// // where
// //     F: Buf + Send,
// // {
// //     type SendFrameFut = GenericBufSender<'a, ChunkedFrames<F>, W>;

// //     fn send_frame(self, frame: F) -> Self::SendFrameFut {
// //         todo!()
// //         // let length = frame.remaining() as u64; // TODO: Try into + handle error.
// //         // let length_prefixed_frame = Bytes::copy_from_slice(&length.to_le_bytes()).chain(frame);
// //         // GenericBufSender::new(length_prefixed_frame, &mut self.writer)
// //     }
// // }
