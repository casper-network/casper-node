//! Splits frames into fragments.
//!
//! The wire format for fragments is `NCCC...` where `CCC...` is the data fragment and `N` is the
//! continuation byte, which is `0x00` if more fragments are following, `0xFF` if this is the
//! frame's last fragment.

use std::{
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use futures::{ready, Sink, SinkExt, Stream, StreamExt};
use thiserror::Error;

use crate::{error::Error, try_ready, ImmediateFrame};

pub type SingleFragment = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more fragments are following.
const MORE_FRAGMENTS: u8 = 0x00;

/// Final fragment indicator.
const FINAL_FRAGMENT: u8 = 0xFF;

#[derive(Debug)]
pub struct Fragmentizer<S, F> {
    current_frame: Option<F>,
    current_fragment: Option<SingleFragment>,
    sink: S,
    fragment_size: NonZeroUsize,
}

impl<S, F> Fragmentizer<S, F>
where
    S: Sink<SingleFragment> + Unpin,
    F: Buf,
{
    /// Creates a new fragmentizer with the given fragment size.
    pub fn new(fragment_size: NonZeroUsize, sink: S) -> Self {
        Fragmentizer {
            current_frame: None,
            current_fragment: None,
            sink,
            fragment_size,
        }
    }

    fn flush_current_frame(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), <S as Sink<SingleFragment>>::Error>> {
        loop {
            if self.current_fragment.is_some() {
                // There is fragment data to send, attempt to make progress:

                // First, poll the sink until it is ready to accept another item.
                try_ready!(ready!(self.sink.poll_ready_unpin(cx)));

                // Extract the item and push it into the underlying sink.
                try_ready!(self
                    .sink
                    .start_send_unpin(self.current_fragment.take().unwrap()));
            }

            // At this point, `current_fragment` is empty, so we try to create another one.
            if let Some(ref mut current_frame) = self.current_frame {
                let remaining = current_frame.remaining().min(self.fragment_size.into());
                let fragment_data = current_frame.copy_to_bytes(remaining);

                let continuation_byte: u8 = if current_frame.has_remaining() {
                    MORE_FRAGMENTS
                } else {
                    // If it is the last fragment, remove the current frame.
                    self.current_frame = None;
                    FINAL_FRAGMENT
                };

                self.current_fragment =
                    Some(ImmediateFrame::from(continuation_byte).chain(fragment_data));
            } else {
                // All our fragments are buffered and there are no more fragments to create.
                return Poll::Ready(Ok(()));
            }
        }
    }
}

impl<F, S> Sink<F> for Fragmentizer<S, F>
where
    F: Buf + Send + Sync + 'static + Unpin,
    S: Sink<SingleFragment> + Unpin,
{
    type Error = <S as Sink<SingleFragment>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        // We will be ready to accept another item once the current one has been flushed fully.
        self_mut.flush_current_frame(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let self_mut = self.get_mut();

        debug_assert!(self_mut.current_frame.is_none());
        self_mut.current_frame = Some(item);

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        try_ready!(ready!(self_mut.flush_current_frame(cx)));

        // At this point everything has been buffered, so we defer to the underlying sink's flush to
        // ensure the final fragment also has been sent.

        self_mut.poll_flush_unpin(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        try_ready!(ready!(self_mut.flush_current_frame(cx)));

        self_mut.poll_close_unpin(cx)
    }
}

#[derive(Debug)]
pub struct Defragmentizer<S> {
    stream: S,
    buffer: BytesMut,
    max_output_frame_size: usize,
}

impl<S> Defragmentizer<S> {
    pub fn new(max_output_frame_size: usize, stream: S) -> Self {
        Defragmentizer {
            stream,
            buffer: BytesMut::new(),
            max_output_frame_size,
        }
    }
}

#[derive(Debug, Error)]
pub enum DefragmentizerError<StreamErr> {
    /// A fragment header was sent that is not `MORE_FRAGMENTS` or `FINAL_FRAGMENT`.
    #[error(
        "received invalid fragment header of {}, expected {} or {}",
        0,
        MORE_FRAGMENTS,
        FINAL_FRAGMENT
    )]
    InvalidFragmentHeader(u8),
    /// A fragment with a length of zero was received that was not final, which is not allowed to
    /// prevent spam with this kind of frame.
    #[error("received fragment with zero length that was not final")]
    NonFinalZeroLengthFragment,
    /// A zero-length fragment (including the envelope) was received, i.e. missing the header.
    #[error("missing fragment header")]
    MissingFragmentHeader,
    /// The incoming stream was closed, with data still in the buffer, missing a final fragment.
    #[error("stream closed mid-frame")]
    IncompleteFrame,
    /// Reading the next fragment would cause the frame to exceed the maximum size.
    #[error("would exceed maximum frame size of {max}")]
    MaximumFrameSizeExceeded {
        /// The configure maximum frame size.
        max: usize,
    },
    /// An error in the underlying transport stream.
    #[error(transparent)]
    Io(StreamErr),
}

impl<S, E> Stream for Defragmentizer<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::error::Error,
{
    type Item = Result<Bytes, DefragmentizerError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();
        loop {
            match ready!(self_mut.stream.poll_next_unpin(cx)) {
                Some(Ok(mut next_fragment)) => {
                    let is_final = match next_fragment.get(0).cloned() {
                        Some(MORE_FRAGMENTS) => true,
                        Some(FINAL_FRAGMENT) => false,
                        Some(invalid) => {
                            return Poll::Ready(Some(Err(
                                DefragmentizerError::InvalidFragmentHeader(invalid),
                            )));
                        }
                        None => {
                            return Poll::Ready(Some(Err(
                                DefragmentizerError::MissingFragmentHeader,
                            )))
                        }
                    };
                    next_fragment.advance(1);

                    // We do not allow 0-length continuation frames to prevent DOS attacks.
                    if next_fragment.is_empty() && !is_final {
                        return Poll::Ready(Some(Err(
                            DefragmentizerError::NonFinalZeroLengthFragment,
                        )));
                    }

                    // Check if we exceeded the maximum buffer.
                    if self_mut.buffer.len() + next_fragment.remaining()
                        > self_mut.max_output_frame_size
                    {
                        return Poll::Ready(Some(Err(
                            DefragmentizerError::MaximumFrameSizeExceeded {
                                max: self_mut.max_output_frame_size,
                            },
                        )));
                    }

                    self_mut.buffer.extend(next_fragment);

                    if is_final {
                        let frame = self_mut.buffer.split().freeze();
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }
                Some(Err(err)) => return Poll::Ready(Some(Err(DefragmentizerError::Io(err)))),
                None => {
                    if self_mut.buffer.is_empty() {
                        // All good, stream just closed.
                        return Poll::Ready(None);
                    } else {
                        return Poll::Ready(Some(Err(DefragmentizerError::IncompleteFrame)));
                    }
                }
            }
        }
    }
}

/// Splits a frame into ready-to-send fragments.
///
/// # Notes
///
/// Internally, data is copied into fragments by using `Buf::copy_to_bytes`. It is advisable to use
/// a `B` that has an efficient implementation for this that avoids copies, like `Bytes` itself.
pub fn fragment_frame<B: Buf>(
    mut frame: B,
    fragment_size: NonZeroUsize,
) -> Result<impl Iterator<Item = SingleFragment>, Error> {
    let fragment_size: usize = fragment_size.into();
    let num_frames = (frame.remaining() + fragment_size - 1) / fragment_size;

    Ok((0..num_frames).into_iter().map(move |_| {
        let remaining = frame.remaining().min(fragment_size);
        let fragment_data = frame.copy_to_bytes(remaining);

        let continuation_byte: u8 = if frame.has_remaining() {
            MORE_FRAGMENTS
        } else {
            FINAL_FRAGMENT
        };
        ImmediateFrame::from(continuation_byte).chain(fragment_data)
    }))
}

#[cfg(test)]
mod tests {

    // #[test]
    // fn basic_fragmenting_works() {
    //     let frame = b"01234567890abcdefghijklmno";

    //     let sink: Vec< = Vec::new();

    //     let fragments: Vec<_> = fragment_frame(&frame[..], 7.try_into().unwrap())
    //         .expect("fragmenting failed")
    //         .map(collect_buf)
    //         .collect();

    //     assert_eq!(
    //         fragments,
    //         vec![
    //             b"\x000123456".to_vec(),
    //             b"\x007890abc".to_vec(),
    //             b"\x00defghij".to_vec(),
    //             b"\xffklmno".to_vec(),
    //         ]
    //     );

    //     // Try with a fragment size that ends exactly on the frame boundary.
    //     let frame = b"012345";
    //     let fragments: Vec<_> = fragment_frame(&frame[..], 3.try_into().unwrap())
    //         .expect("fragmenting failed")
    //         .map(collect_buf)
    //         .collect();

    //     assert_eq!(fragments, vec![b"\x00012".to_vec(), b"\xff345".to_vec(),]);
    // }

    // #[test]
    // fn fragmenting_for_small_size_works() {
    //     let frame = b"012345";
    //     let fragments: Vec<_> = fragment_frame(&frame[..], 6.try_into().unwrap())
    //         .expect("fragmenting failed")
    //         .map(collect_buf)
    //         .collect();

    //     assert_eq!(fragments, vec![b"\xff012345".to_vec()]);

    //     // Try also with mismatched fragment size.
    //     let fragments: Vec<_> = fragment_frame(&frame[..], 15.try_into().unwrap())
    //         .expect("fragmenting failed")
    //         .map(collect_buf)
    //         .collect();

    //     assert_eq!(fragments, vec![b"\xff012345".to_vec()]);
    // }
}
