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

use crate::{try_ready, ImmediateFrame};

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

        self_mut.sink.poll_flush_unpin(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();

        try_ready!(ready!(self_mut.flush_current_frame(cx)));

        self_mut.sink.poll_close_unpin(cx)
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
                        Some(MORE_FRAGMENTS) => false,
                        Some(FINAL_FRAGMENT) => true,
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

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, num::NonZeroUsize, sync::Arc};

    use bytes::{Buf, Bytes};
    use futures::{channel::mpsc, FutureExt, SinkExt, StreamExt};

    use crate::{
        fragmented::{Defragmentizer, DefragmentizerError},
        testing::testing_sink::TestingSink,
    };

    use super::{Fragmentizer, SingleFragment};

    const CHANNEL_BUFFER_SIZE: usize = 1000;

    impl<E> PartialEq for DefragmentizerError<E> {
        fn eq(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::InvalidFragmentHeader(l0), Self::InvalidFragmentHeader(r0)) => l0 == r0,
                (
                    Self::MaximumFrameSizeExceeded { max: l_max },
                    Self::MaximumFrameSizeExceeded { max: r_max },
                ) => l_max == r_max,
                (Self::Io(_), Self::Io(_)) => true,
                _ => core::mem::discriminant(self) == core::mem::discriminant(other),
            }
        }
    }

    #[test]
    fn fragmenter_basic() {
        const FRAGMENT_SIZE: usize = 8;

        let testing_sink = Arc::new(TestingSink::new());
        let mut fragmentizer = Fragmentizer::new(
            NonZeroUsize::new(FRAGMENT_SIZE).unwrap(),
            testing_sink.clone().into_ref(),
        );

        let frame_data = b"01234567890abcdefghijklmno";
        let frame = Bytes::from(frame_data.to_vec());

        fragmentizer
            .send(frame)
            .now_or_never()
            .expect("fragmentizer was pending")
            .expect("fragmentizer failed");

        let contents = testing_sink.get_contents();
        assert_eq!(contents, b"\x0001234567\x00890abcde\x00fghijklm\xFFno");
    }

    #[test]
    fn defragmentizer_basic() {
        let frame_data = b"01234567890abcdefghijklmno";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        let frames: Vec<Bytes> = defragmentizer
            .map(|bytes_result| bytes_result.unwrap())
            .collect()
            .now_or_never()
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], frame_data.as_slice());
    }

    #[test]
    fn fragment_roundtrip() {
        const FRAGMENT_SIZE: usize = 8;
        let original_frame = b"01234567890abcdefghijklmno";
        let frame_vec = original_frame.to_vec();
        let frame = Bytes::from(frame_vec);
        let (sender, receiver) = mpsc::channel::<SingleFragment>(CHANNEL_BUFFER_SIZE);

        {
            let mut fragmentizer = Fragmentizer::new(FRAGMENT_SIZE.try_into().unwrap(), sender);
            fragmentizer
                .send(frame.clone())
                .now_or_never()
                .expect("Couldn't send frame")
                .unwrap();
            fragmentizer
                .flush()
                .now_or_never()
                .expect("Couldn't flush sender")
                .unwrap();
        }

        let receiver = receiver.map(|mut fragment| {
            let item: Result<Bytes, DefragmentizerError<Infallible>> =
                Ok(fragment.copy_to_bytes(fragment.remaining()));
            item
        });

        let defragmentizer = Defragmentizer::new(original_frame.len(), receiver);
        let frames: Vec<Bytes> = defragmentizer
            .map(|bytes_result| bytes_result.unwrap())
            .collect()
            .now_or_never()
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], original_frame.as_slice());
    }

    #[test]
    fn defragmentizer_incomplete_frame() {
        let frame_data = b"01234567890abcdefghijklmno";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        // Send just 2 frames and prematurely close the stream.
        sender
            .send(Ok(fragments[0].clone()))
            .now_or_never()
            .expect("Couldn't send encoded frame")
            .unwrap();
        sender
            .send(Ok(fragments[1].clone()))
            .now_or_never()
            .expect("Couldn't send encoded frame")
            .unwrap();
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let mut defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        // Ensure we don't incorrectly yield a frame.
        assert_eq!(
            defragmentizer
                .next()
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DefragmentizerError::IncompleteFrame
        );
    }

    #[test]
    fn defragmentizer_invalid_fragment_header() {
        let frame_data = b"01234567890abcdefghijklmno";
        // Insert invalid header '0xAB' into the first fragment.
        let mut fragments: Vec<Bytes> = [b"\xAB01234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let mut defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        assert_eq!(
            defragmentizer
                .next()
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DefragmentizerError::InvalidFragmentHeader(0xAB)
        );
    }

    #[test]
    fn defragmentizer_zero_length_non_final_fragment() {
        let frame_data = b"01234567890abcdefghijklmno";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        // Insert an empty, non-final fragment with just the header.
        fragments.push(b"\x00".as_slice().into());
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let mut defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        assert_eq!(
            defragmentizer
                .next()
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DefragmentizerError::NonFinalZeroLengthFragment
        );
    }

    #[test]
    fn defragmentizer_zero_length_final_fragment() {
        let frame_data = b"01234567890abcdefghijklm";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        // Insert an empty, final fragment with just the header. This should
        // succeed as the requirement to have non-empty fragments only applies
        // to non-final fragments.
        fragments.push(b"\xFF".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        let frames: Vec<Bytes> = defragmentizer
            .map(|bytes_result| bytes_result.unwrap())
            .collect()
            .now_or_never()
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], frame_data.as_slice());
    }

    #[test]
    fn defragmentizer_missing_fragment_header() {
        let frame_data = b"01234567890abcdefghijklmno";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        // Insert an empty fragment, not even a header in it.
        fragments.push(b"".as_slice().into());
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        let mut defragmentizer = Defragmentizer::new(frame_data.len(), receiver);
        assert_eq!(
            defragmentizer
                .next()
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DefragmentizerError::MissingFragmentHeader
        );
    }

    #[test]
    fn defragmentizer_max_frame_size_exceeded() {
        let frame_data = b"01234567890abcdefghijklmno";
        let mut fragments: Vec<Bytes> = [b"\x0001234567", b"\x00890abcde", b"\x00fghijklm"]
            .into_iter()
            .map(|bytes| bytes.as_slice().into())
            .collect();
        fragments.push(b"\xFFno".as_slice().into());

        let (mut sender, receiver) =
            mpsc::channel::<Result<Bytes, Infallible>>(CHANNEL_BUFFER_SIZE);
        for fragment in fragments {
            sender
                .send(Ok(fragment))
                .now_or_never()
                .expect("Couldn't send encoded frame")
                .unwrap();
        }
        sender
            .flush()
            .now_or_never()
            .expect("Couldn't flush")
            .unwrap();
        drop(sender);

        // Initialize the defragmentizer with a max frame length lower than what
        // we're trying to send.
        let mut defragmentizer = Defragmentizer::new(frame_data.len() - 1, receiver);
        // Ensure the data doesn't fit in the frame size limit.
        assert_eq!(
            defragmentizer
                .next()
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DefragmentizerError::MaximumFrameSizeExceeded {
                max: frame_data.len() - 1
            }
        );
    }
}
