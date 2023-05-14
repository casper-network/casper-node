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

#[derive(Debug)]
struct SegmentInfo {
    total_payload_length: u32,
    start: NonZeroU8,
    payload_segment_len: u32,
}

impl SegmentInfo {
    fn is_complete(&self) -> bool {
        self.total_payload_length == self.payload_segment_len
    }
}

#[derive(Copy, Clone, Debug)]
enum SegmentError {
    ExceedsMaxPayloadLength,
    BadVarInt,
}

/// Given a potential segment buffer (which is a frame without the header), finds a start segment.
///
/// Assumes that the first bytes of the buffer are a [`crate::varint`] encoded length.
fn find_start_segment(
    segment_buf: &[u8],
    max_payload_length: u32,
    max_frame_size: u32,
) -> Outcome<SegmentInfo, SegmentError> {
    let ParsedU32 {
        offset,
        value: total_payload_length,
    } = try_outcome!(decode_varint32(segment_buf).map_err(|_| SegmentError::BadVarInt));

    // Ensure it is within allowed range.
    if total_payload_length > max_payload_length {
        return Err(SegmentError::ExceedsMaxPayloadLength);
    }

    // We have a valid length. Calculate how much space there is in this frame and determine whether or not our payload would fit entirely into the start segment.
    let full_payload_size = max_frame_size - (offset.get() as u32 + Header::SIZE as u32);
    if total_payload_length <= full_payload_size {
        // The entire payload fits into the segment. Check if we have enough. Do all math in 64 bit,
        // since we have to assume that `total_payload_length` can be up to [`u32::MAX`].

        if segment_buf.len() as u64 >= total_payload_length as u64 + offset.get() as u64 {
            Success(SegmentInfo {
                total_payload_length,
                start: offset,
                payload_segment_len: total_payload_length,
            })
        } else {
            // The payload would fit, but we do not have enough data yet.
            Incomplete(
                NonZeroU32::new(
                    total_payload_length - segment_buf.len() as u32 + offset.get() as u32,
                )
                .unwrap(),
            )
        }
    } else {
        // The entire frame must be filled according to the RFC.
        let actual_payload_len = segment_buf.len() - offset.get() as usize;
        if actual_payload_len < full_payload_size as usize {
            Incomplete(NonZeroU32::new(full_payload_size - actual_payload_len as u32).unwrap())
        } else {
            // Frame is full.
            Success(SegmentInfo {
                total_payload_length,
                start: offset,
                payload_segment_len: full_payload_size,
            })
        }
    }
}
