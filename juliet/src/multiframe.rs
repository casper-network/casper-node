use std::num::{NonZeroU32, NonZeroU8};

use bytes::{Buf, Bytes, BytesMut};

use crate::{
    header::{ErrorKind, Header},
    try_outcome,
    varint::{decode_varint32, ParsedU32},
    Outcome::{self, Err, Incomplete, Success},
};

/// A multi-frame message reader.
///
/// Processes frames into message from a given input stream as laid out in the juliet RFC.
#[derive(Debug)]
pub(crate) enum MultiFrameReader {
    Ready,
    InProgress { header: Header, payload: BytesMut },
}

impl MultiFrameReader {
    /// Accept additional data to be written.
    ///
    /// Assumes that `header` is the first [`Header::SIZE`] bytes of `buffer`. Will advance `buffer`
    /// past header and payload if and only a successful frame was parsed.
    ///
    /// Continues parsing until either a complete message is found or additional input is required.
    /// Will return the message payload associated with the passed in `header`, if complete.
    ///
    /// # Panics
    ///
    /// Panics when compiled with debug settings if `max_frame_size` is less than 10 or `buffer` is
    /// shorter than [`Header::SIZE`].
    pub(crate) fn accept(
        &mut self,
        header: Header,
        buffer: &mut BytesMut,
        max_frame_size: u32,
    ) -> Outcome<BytesMut, Header> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );
        debug_assert!(
            buffer.len() >= Header::SIZE,
            "buffer is too small to contain header"
        );

        let segment_buf = &buffer[0..Header::SIZE];

        match self {
            MultiFrameReader::InProgress {
                header: pheader,
                payload,
            } if *pheader == header => {
                todo!("this is the case where we are appending to a message")
            }
            MultiFrameReader::InProgress { .. } | MultiFrameReader::Ready => {
                // We have a new segment, which has a variable size.
                let ParsedU32 {
                    offset,
                    value: total_payload_size,
                } =
                    try_outcome!(decode_varint32(segment_buf)
                        .map_err(|_| header.with_err(ErrorKind::BadVarInt)));

                // We have a valid varint32. Let's see if we're inside the frame boundary.
                let preamble_size = Header::SIZE as u32 + offset.get() as u32;
                let max_data_in_frame = (max_frame_size - preamble_size) as u32;

                // Drop header and length.
                buffer.advance(preamble_size as usize);
                if total_payload_size <= max_data_in_frame {
                    let payload = buffer.split_to(total_payload_size as usize);

                    // No need to alter the state, we stay `Ready`.
                    return Success(payload);
                }

                // The length exceeds the frame boundary, split to maximum and store that.
                let partial_payload = buffer.split_to((max_frame_size - preamble_size) as usize);

                *self = MultiFrameReader::InProgress {
                    header,
                    payload: partial_payload,
                };

                // TODO: THIS IS WRONG. LOOP READING. AND CONSIDER ACTUAL BUFFER LENGTH
                // ABOVE. We need at least a header to proceed further on.
                return Incomplete(NonZeroU32::new(Header::SIZE as u32).unwrap());

                todo!()
            }
            MultiFrameReader::InProgress { header, payload } => todo!(),
            _ => todo!(),
        }
    }
}

/// Information about the payload of a starting segment.
#[derive(Debug)]
struct PayloadInfo {
    /// Total size of the entire message's payload (across all frames).
    message_length: u32,
    /// Start of the payload, relative to segment start.
    start: NonZeroU8,
    /// End of the payload, relative to segment start.
    end: u32,
}

impl PayloadInfo {
    /// Returns the length of the payload in the segment.
    #[inline(always)]
    fn len(&self) -> u32 {
        self.end - self.start.get() as u32
    }

    /// Returns whether the entire message payload is contained in the starting segment.
    #[inline(always)]
    fn is_complete(&self) -> bool {
        self.message_length == self.len()
    }
}

/// Error parsing starting segment.
#[derive(Copy, Clone, Debug)]
enum SegmentError {
    /// The advertised message payload length exceeds the configured limit.
    ExceedsMaxPayloadLength,
    /// The varint at the beginning could not be parsed.
    BadVarInt,
}

/// Given a potential segment buffer (which is a frame without the header), finds a start segment.
///
/// Assumes that the first bytes of the buffer are a [`crate::varint`] encoded length. Returns the
/// geometry of the segment that was found.
fn find_start_segment(
    segment_buf: &[u8],
    max_payload_length: u32,
    max_frame_size: u32,
) -> Outcome<PayloadInfo, SegmentError> {
    let ParsedU32 {
        offset: start,
        value: message_length,
    } = try_outcome!(decode_varint32(segment_buf).map_err(|_| SegmentError::BadVarInt));

    // Ensure it is within allowed range.
    if message_length > max_payload_length {
        return Err(SegmentError::ExceedsMaxPayloadLength);
    }

    // Determine the largest payload that can still fit into this frame.
    let full_payload_size = max_frame_size - (start.get() as u32 + Header::SIZE as u32);

    // Calculate start and end of payload in this frame, the latter capped by the frame itself.
    let end = start.get() as u32 + full_payload_size.min(message_length);

    // Determine if segment is complete.
    if end as usize > segment_buf.len() {
        let missing = segment_buf.len() - end as usize;
        // Note: Missing is guaranteed to be <= `u32::MAX` here.
        Incomplete(NonZeroU32::new(missing as u32).unwrap())
    } else {
        Success(PayloadInfo {
            message_length,
            start,
            end,
        })
    }
}
