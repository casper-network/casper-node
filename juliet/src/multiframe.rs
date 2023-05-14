use std::{
    mem,
    num::{NonZeroU32, NonZeroU8},
};

use bytes::{Buf, BytesMut};

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
    InProgress {
        header: Header,
        msg_payload: BytesMut,
        msg_len: u32,
    },
}

impl MultiFrameReader {
    /// Process a single frame from a buffer.
    ///
    /// Assumes that `header` was the first [`Header::SIZE`] preceding `buffer`. Will advance
    /// `buffer` past header and payload if and only a successful frame was parsed.
    ///
    /// Returns a completed message payload, or `None` if a frame was consumed, but no message
    /// completed yet.
    ///
    /// # Panics
    ///
    /// Panics when compiled with debug settings if `max_frame_size` is less than 10 or `buffer` is
    /// shorter than [`Header::SIZE`].
    pub(crate) fn process_frame(
        &mut self,
        header: Header,
        buffer: &mut BytesMut,
        max_payload_length: u32,
        max_frame_size: u32,
    ) -> Outcome<Option<BytesMut>, Header> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );
        debug_assert!(
            buffer.len() >= Header::SIZE,
            "buffer is too small to contain header"
        );

        // Check if we got a continuation of a message send already in progress.
        match self {
            MultiFrameReader::InProgress {
                header: pheader,
                msg_payload,
                msg_len,
            } if *pheader == header => {
                let max_frame_payload = max_frame_size - Header::SIZE as u32;
                let remaining = (*msg_len - msg_payload.len() as u32).min(max_frame_payload);

                // If we don't have enough data yet, return number of bytes missing.
                let end = (remaining as u64 + Header::SIZE as u64);
                if buffer.len() < end as usize {
                    return Incomplete(
                        NonZeroU32::new((end - buffer.len() as u64) as u32).unwrap(),
                    );
                }

                // Otherwise, we're good to append to the payload.
                msg_payload.extend_from_slice(&buffer[Header::SIZE..(end as usize)]);
                msg_payload.advance(end as usize);

                return Success(if remaining < max_frame_payload {
                    let rv = mem::take(msg_payload);
                    *self = MultiFrameReader::Ready;
                    Some(rv)
                } else {
                    None
                });
            }
            _ => (),
        }

        // At this point we have to expect a starting segment.
        let payload_info = try_outcome!(find_start_segment(
            &buffer[Header::SIZE..],
            max_payload_length,
            max_frame_size
        )
        .map_err(|err| err.into_header()));

        // Discard the header and length, then split off the payload.
        buffer.advance(Header::SIZE + payload_info.start.get() as usize);
        let segment_payload = buffer.split_to(payload_info.len() as usize);

        // We can finally determine our outcome.
        match self {
            MultiFrameReader::InProgress { .. } => {
                if !payload_info.is_complete() {
                    Err(header.with_err(ErrorKind::InProgress))
                } else {
                    Success(Some(segment_payload))
                }
            }
            MultiFrameReader::Ready => {
                if !payload_info.is_complete() {
                    // Begin a new multi-frame read.
                    *self = MultiFrameReader::InProgress {
                        header,
                        msg_payload: segment_payload,
                        msg_len: payload_info.message_length,
                    };
                    // The next minimum read is another header.
                    Incomplete(NonZeroU32::new(Header::SIZE as u32).unwrap())
                } else {
                    // The entire message is contained, no need to change state.
                    Success(Some(segment_payload))
                }
            }
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

impl SegmentError {
    fn into_header(self) -> Header {
        match self {
            SegmentError::ExceedsMaxPayloadLength => todo!(),
            SegmentError::BadVarInt => todo!(),
        }
    }
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
