//! Splits frames into fragments.
//!
//! The wire format for fragments is `NCCC...` where `CCC...` is the data fragment and `N` is the
//! continuation byte, which is `0x00` if more fragments are following, `0xFF` if this is the frame's
//! last fragment.

use std::{
    future, io,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{
    ready,
    stream::{self},
    Sink, SinkExt, Stream, StreamExt,
};

use crate::{error::Error, try_ready, ImmediateFrame};

pub type SingleFragment = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more fragments are following.
const MORE_FRAGMENTS: u8 = 0x00;

/// Final fragment indicator.
const FINAL_FRAGMENT: u8 = 0xFF;

#[derive(Debug)]
struct Fragmentizer<S, F> {
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

/// Splits a frame into ready-to-send fragments.
///
/// # Notes
///
/// Internally, data is copied into fragments by using `Buf::copy_to_bytes`. It is advisable to use a
/// `B` that has an efficient implementation for this that avoids copies, like `Bytes` itself.
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

/// Generates the "fragmentizer", i.e: an object that when given the source stream of bytes will yield single fragments.
#[allow(unused)]
pub(crate) fn make_fragmentizer<S, E>(
    sink: S,
    fragment_size: NonZeroUsize,
) -> impl Sink<Bytes, Error = io::Error>
where
    E: std::error::Error,
    S: Sink<SingleFragment, Error = io::Error>,
{
    sink.with_flat_map(move |frame: Bytes| {
        let fragment_iter = fragment_frame(frame, fragment_size).expect("TODO: Handle error");
        stream::iter(fragment_iter.map(Result::<_, _>::Ok))
    })
}

/// Generates the "defragmentizer", i.e.: an object that when given the source stream of fragments will yield the complete message.
#[allow(unused)]
pub(crate) fn make_defragmentizer<S: Stream<Item = std::io::Result<Bytes>>>(
    source: S,
) -> impl Stream<Item = Bytes> {
    let mut buffer = vec![];
    source.filter_map(move |fragment| {
        let mut fragment = fragment.expect("TODO: handle read error");
        let first_byte = *fragment.first().expect("missing first byte");
        buffer.push(fragment.split_off(1));
        match first_byte {
            FINAL_FRAGMENT => {
                // TODO: Check the true zero-copy approach.
                let mut buf = BytesMut::new();
                for fragment in buffer.drain(..) {
                    buf.put_slice(&fragment);
                }
                future::ready(Some(buf.freeze()))
            }
            MORE_FRAGMENTS => future::ready(None),
            _ => panic!("garbage found where continuation byte was expected"),
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::tests::collect_buf;

    use super::fragment_frame;

    #[test]
    fn basic_fragmenting_works() {
        let frame = b"01234567890abcdefghijklmno";

        let fragments: Vec<_> = fragment_frame(&frame[..], 7.try_into().unwrap())
            .expect("fragmenting failed")
            .map(collect_buf)
            .collect();

        assert_eq!(
            fragments,
            vec![
                b"\x000123456".to_vec(),
                b"\x007890abc".to_vec(),
                b"\x00defghij".to_vec(),
                b"\xffklmno".to_vec(),
            ]
        );

        // Try with a fragment size that ends exactly on the frame boundary.
        let frame = b"012345";
        let fragments: Vec<_> = fragment_frame(&frame[..], 3.try_into().unwrap())
            .expect("fragmenting failed")
            .map(collect_buf)
            .collect();

        assert_eq!(fragments, vec![b"\x00012".to_vec(), b"\xff345".to_vec(),]);
    }

    #[test]
    fn fragmenting_for_small_size_works() {
        let frame = b"012345";
        let fragments: Vec<_> = fragment_frame(&frame[..], 6.try_into().unwrap())
            .expect("fragmenting failed")
            .map(collect_buf)
            .collect();

        assert_eq!(fragments, vec![b"\xff012345".to_vec()]);

        // Try also with mismatched fragment size.
        let fragments: Vec<_> = fragment_frame(&frame[..], 15.try_into().unwrap())
            .expect("fragmenting failed")
            .map(collect_buf)
            .collect();

        assert_eq!(fragments, vec![b"\xff012345".to_vec()]);
    }
}
