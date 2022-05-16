//! Length prefixed chunking.
//!
//! Prefixes frames with their length, which is hard coded at 16 bit little endian ints.

use bytes::Buf;

use crate::{error::Error, ImmediateFrame};

/// A frame that has had a length prefix added.
pub type LengthPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 2]>, F>;

/// Adds a length prefix to the given frame.
pub fn frame_add_length_prefix<F: Buf, E: std::error::Error>(
    frame: F,
) -> Result<LengthPrefixedFrame<F>, Error<E>> {
    let remaining = frame.remaining();
    let length: u16 = remaining.try_into().map_err(|_err| Error::FrameTooLong {
        actual: remaining,
        max: u16::MAX as usize,
    })?;
    Ok(ImmediateFrame::from(length).chain(frame))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use crate::{error::Error, tests::collect_buf};

    use super::frame_add_length_prefix;

    #[test]
    fn length_prefixing_of_single_frame_works() {
        let frame = &b"abcdefg"[..];
        let prefixed = frame_add_length_prefix::<_, Infallible>(frame).expect("prefixing failed");

        let output = collect_buf(prefixed);
        assert_eq!(output, b"\x07\x00abcdefg");
    }

    #[test]
    fn large_frames_reject() {
        let frame = [0; 1024 * 1024];
        let result = frame_add_length_prefix::<_, Infallible>(&frame[..]);

        assert!(matches!(
            result,
            Err(Error::FrameTooLong { actual, max })
                if actual == frame.len() && max == u16::MAX as usize
        ))
    }
}
