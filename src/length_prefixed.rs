use bytes::Buf;
use thiserror::Error;

use crate::ImmediateFrame;

/// A frame prefix conversion error.
#[derive(Debug, Error)]
pub enum Error {
    /// The frame's length cannot be represented with the prefix.
    #[error("frame too long {actual}/{max}")]
    FrameTooLong { actual: usize, max: usize },
}

/// A frame that has had a length prefix added.
pub type LengthPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 2]>, F>;

/// Adds a length prefix to the given frame.
pub fn frame_add_length_prefix<F: Buf>(frame: F) -> Result<LengthPrefixedFrame<F>, Error> {
    let remaining = frame.remaining();
    let length: u16 = remaining.try_into().map_err(|_err| Error::FrameTooLong {
        actual: remaining,
        max: u16::MAX as usize,
    })?;
    Ok(ImmediateFrame::from(length).chain(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use bytes::Buf;

    use super::frame_add_length_prefix;

    #[test]
    fn length_prefixing_of_single_frame_works() {
        let frame = &b"abcdefg"[..];
        let prefixed = frame_add_length_prefix(frame).expect("prefixing failed");

        let mut output = Vec::new();
        prefixed
            .reader()
            .read_to_end(&mut output)
            .expect("failed to read");

        assert_eq!(output, b"\x07\x00abcdefg");
    }
}
