use std::{
    default, mem,
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
#[derive(Debug, Default)]
pub(crate) enum MultiFrameReader {
    #[default]
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
    /// Panics when compiled with debug profiles if `max_frame_size` is less than 10 or `buffer` is
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

                return Success(if remaining <= max_frame_payload {
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
        let missing = end as usize - segment_buf.len();

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

#[cfg(test)]
mod tests {
    use std::{io::Write, num::NonZeroU32};

    use bytes::{Buf, BufMut, BytesMut};
    use proptest::{collection::vec, prelude::any, proptest};

    use crate::{
        header::{
            Header,
            Kind::{self, RequestPl},
        },
        multiframe::{PayloadInfo, SegmentError},
        varint::Varint32,
        ChannelId, Id, Outcome,
    };

    use super::{find_start_segment, MultiFrameReader};

    const FRAME_MAX_PAYLOAD: usize = 500;
    const MAX_FRAME_SIZE: usize =
        FRAME_MAX_PAYLOAD + Header::SIZE + Varint32::encode(FRAME_MAX_PAYLOAD as u32).len();

    proptest! {
        #[test]
        fn single_frame_message(payload in vec(any::<u8>(), FRAME_MAX_PAYLOAD), garbage in vec(any::<u8>(), 10)) {
            do_single_frame_messages(payload, garbage);
        }
    }

    #[test]
    fn find_start_segment_simple_cases() {
        // Empty case should return 1.
        assert!(matches!(
            find_start_segment(&[], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Incomplete(n) if n.get() == 1
        ));

        // With a length 0, we should get a result after 1 byte.
        assert!(matches!(
            find_start_segment(&[0x00], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 0,
                start,
                end: 1
            }) if start.get() == 1
        ));

        // Additional byte should return the correct amount of extra required bytes.
        assert!(matches!(
            find_start_segment(&[0x7], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Incomplete(n) if n.get() == 7
        ));
        assert!(matches!(
            find_start_segment(&[0x7, 0xA0, 0xA1, 0xA2], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Incomplete(n) if n.get() == 4
        ));
        assert!(matches!(
            find_start_segment(&[0x7, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Incomplete(n) if n.get() == 1
        ));
        assert!(matches!(
            find_start_segment(&[0x7, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 7,
                start,
                end: 8
            }) if start.get() == 1
        ));

        // We can also check if additional data is ignored properly.
        assert!(matches!(
            find_start_segment(&[0x7, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xEE], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 7,
                start,
                end: 8
            }) if start.get() == 1
        ));
        assert!(matches!(
            find_start_segment(&[0x7, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xEE, 0xEE, 0xEE,
                0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE], FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 7,
                start,
                end: 8
            }) if start.get() == 1
        ));

        // Finally, try with larger value (that doesn't fit into length encoding of 1).
        // 0x83 0x01 == 0b1000_0011 = 131.
        let mut buf = vec![0x83, 0x01, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];

        assert!(matches!(
            find_start_segment(&buf, FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Incomplete(n) if n.get() == 126
        ));
        buf.extend(vec![0xFF; 126]);
        assert!(matches!(
            find_start_segment(&buf, FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 131,
                start,
                end: 133
            }) if start.get() == 2
        ));
        buf.extend(vec![0x77; 999]);
        assert!(matches!(
            find_start_segment(&buf, FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Success(PayloadInfo {
                message_length: 131,
                start,
                end: 133
            }) if start.get() == 2
        ));
    }

    #[test]
    fn find_start_segment_errors() {
        let bad_varint = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(matches!(
            find_start_segment(&bad_varint, FRAME_MAX_PAYLOAD as u32, MAX_FRAME_SIZE as u32),
            Outcome::Err(SegmentError::BadVarInt)
        ));

        // We expect the size error to be reported immediately, not after parsing the frame.
        let exceeds_size = [0x09];
        assert!(matches!(
            find_start_segment(&exceeds_size, 8, MAX_FRAME_SIZE as u32),
            Outcome::Err(SegmentError::ExceedsMaxPayloadLength)
        ));
        // This should happen regardless of the maximum frame being larger or smaller than the
        // maximum payload.
        assert!(matches!(
            find_start_segment(&exceeds_size, 8, 4),
            Outcome::Err(SegmentError::ExceedsMaxPayloadLength)
        ));
    }

    fn do_single_frame_messages(payload: Vec<u8>, garbage: Vec<u8>) {
        let buffer = BytesMut::new();
        let mut writer = buffer.writer();

        let chan = ChannelId::new(2);
        let id = Id::new(12345);

        let header = Header::new(RequestPl, chan, id);

        // Manually prepare a suitable message buffer.
        writer.write_all(header.as_ref()).unwrap();
        writer
            .write_all(Varint32::encode(payload.len() as u32).as_ref())
            .unwrap();
        writer.write_all(&payload).unwrap();

        let buffer = writer.into_inner();
        // Sanity check constraints.
        if payload.len() == FRAME_MAX_PAYLOAD {
            assert_eq!(buffer.len(), MAX_FRAME_SIZE);
        }
        let mut writer = buffer.writer();

        // Append some random garbage.
        writer.write_all(&garbage).unwrap();

        // Buffer is now ready to read.
        let mut buffer = writer.into_inner();

        // Now we can finally attempt to read it.
        let mut state = MultiFrameReader::default();
        let output = state
            .process_frame(
                header,
                &mut buffer,
                FRAME_MAX_PAYLOAD as u32,
                MAX_FRAME_SIZE as u32,
            )
            // .expect("failed to read using multi frame reader, expected complete single frame")
            .unwrap()
            .expect("did not expect state of single frame to return `None`");

        assert_eq!(output, payload);
    }

    #[test]
    fn allows_interspersed_messages() {
        todo!()
    }

    #[test]
    fn forbids_exceeding_maximum_message_size() {
        todo!()
    }

    #[test]
    fn bad_varint_causes_error() {
        todo!()
    }

    #[test]
    fn varying_message_sizes() {
        todo!("proptest")
    }

    #[test]
    fn fuzz_multi_frame_reader() {
        todo!()
    }
}
