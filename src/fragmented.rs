//! Splits frames into fragments.
//!
//! The wire format for fragments is `NCCC...` where `CCC...` is the data fragment and `N` is the
//! continuation byte, which is `0x00` if more fragments are following, `0xFF` if this is the frame's
//! last fragment.

use std::{future, io, num::NonZeroUsize};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{
    stream::{self},
    Sink, SinkExt, Stream, StreamExt,
};

use crate::{error::Error, ImmediateFrame};

pub type SingleFragment = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more fragments are following.
const MORE_FRAGMENTS: u8 = 0x00;

/// Final fragment indicator.
const FINAL_FRAGMENT: u8 = 0xFF;

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
