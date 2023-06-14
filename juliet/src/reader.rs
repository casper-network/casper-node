use std::{collections::HashSet, mem};

use bytes::{Buf, Bytes, BytesMut};

use crate::{
    header::{ErrorKind, Header, Kind},
    varint::{decode_varint32, Varint32Result},
    ChannelId, Id,
};

const UNKNOWN_CHANNEL: ChannelId = ChannelId::new(0);
const UNKNOWN_ID: Id = Id::new(0);

#[derive(Debug)]
pub struct State<const N: usize> {
    channels: [Channel; N],
    max_frame_size: u32,
}

#[derive(Debug)]
struct Channel {
    incoming_requests: HashSet<Id>,
    outgoing_requests: HashSet<Id>,
    request_limit: u32,
    max_request_payload_size: u32,
    max_response_payload_size: u32,
    current_request_state: RequestState,
}

#[derive(Debug)]
enum RequestState {
    Ready,
    InProgress {
        header: Header,
        payload: BytesMut,
        total_payload_size: u32,
    },
}

impl RequestState {
    /// Accept additional data to be written.
    ///
    /// If a message payload matching the given header has been succesfully completed, returns it.
    /// If a starting or intermediate segment was processed without completing the message, returns
    /// `None` instead. This method will never consume more than one frame.
    ///
    /// Assumes that `header` is the first [`Header::SIZE`] bytes of `buffer`. Will advance `buffer`
    /// past header and payload only on success.
    fn accept(
        &mut self,
        header: Header,
        buffer: &mut BytesMut,
        max_frame_size: u32,
    ) -> Outcome<Option<BytesMut>> {
        debug_assert!(
            max_frame_size >= 10,
            "maximum frame size must be enough to hold header and varint"
        );

        match self {
            RequestState::Ready => {
                // We have a new segment, which has a variable size.
                let segment_buf = &buffer[Header::SIZE..];

                match decode_varint32(segment_buf) {
                    Varint32Result::Incomplete => return Incomplete(1),
                    Varint32Result::Overflow => return header.return_err(ErrorKind::BadVarInt),
                    Varint32Result::Valid {
                        offset,
                        value: total_payload_size,
                    } => {
                        // We have a valid varint32.
                        let preamble_size = Header::SIZE as u32 + offset.get() as u32;
                        let max_data_in_frame = (max_frame_size - preamble_size) as u32;

                        // Determine how many additional bytes are needed for frame completion.
                        let frame_ends_at = (preamble_size as usize
                            + (max_data_in_frame as usize).min(total_payload_size as usize));
                        if buffer.remaining() < frame_ends_at {
                            return Incomplete(buffer.remaining() - frame_ends_at);
                        }

                        // At this point we are sure to complete a frame, so drop the preamble.
                        buffer.advance(preamble_size as usize);

                        // Pure defensive coding: Drop all now-invalid offsets.
                        // TODO: This has no effect, replace with https://compilersaysno.com/posts/owning-your-invariants/
                        drop(frame_ends_at);
                        drop(preamble_size);

                        // Is the payload complete in one frame?
                        if total_payload_size <= max_data_in_frame {
                            let payload = buffer.split_to(total_payload_size as usize);

                            // No need to alter the state, we stay `Ready`.
                            Success(Some(payload))
                        } else {
                            // Length exceeds the frame boundary, split to maximum and store that.
                            let partial_payload = buffer.split_to(max_frame_size as usize);

                            // We are now in progress of reading a payload.
                            *self = RequestState::InProgress {
                                header,
                                payload: partial_payload,
                                total_payload_size,
                            };

                            // We have successfully consumed a frame, but are not finished yet.
                            Success(None)
                        }
                    }
                }
            }
            RequestState::InProgress {
                header: active_header,
                payload,
                total_payload_size,
            } => {
                if header.kind_byte_without_reserved() != active_header.kind_byte_without_reserved()
                {
                    // The newly supplied header does not match the one active.
                    return header.return_err(ErrorKind::InProgress);
                }

                // Determine whether we expect an intermediate or end segment.
                let bytes_remaining = *total_payload_size as usize - payload.remaining();
                let max_data_in_frame = (max_frame_size as usize - Header::SIZE);

                if bytes_remaining > max_data_in_frame {
                    // Intermediate segment.
                    if buffer.remaining() < max_frame_size as usize {
                        return Incomplete(max_frame_size as usize - buffer.remaining());
                    }

                    // Discard header.
                    buffer.advance(Header::SIZE);

                    // Copy data over to internal buffer.
                    payload.extend_from_slice(&buffer[0..max_data_in_frame]);
                    buffer.advance(max_data_in_frame);

                    // We're done with this frame (but not the payload).
                    Success(None)
                } else {
                    // End segment
                    let frame_end = bytes_remaining + Header::SIZE;

                    // If we don't have the entire frame read yet, return.
                    if frame_end > buffer.remaining() {
                        return Incomplete(frame_end - buffer.remaining());
                    }

                    // Discard header.
                    buffer.advance(Header::SIZE);

                    // Copy data over to internal buffer.
                    payload.extend_from_slice(&buffer[0..bytes_remaining]);
                    buffer.advance(bytes_remaining);

                    let finished_payload = mem::take(payload);
                    *self = RequestState::Ready;

                    Success(Some(finished_payload))
                }
            }
        }
    }
}

impl Channel {
    #[inline]
    fn in_flight_requests(&self) -> u32 {
        self.incoming_requests.len() as u32
    }

    #[inline]
    fn is_at_max_requests(&self) -> bool {
        self.in_flight_requests() == self.request_limit
    }
}

enum CompletedRead {
    ErrorReceived(Header),
    NewRequest { id: Id, payload: Option<Bytes> },
}

enum Outcome<T> {
    Incomplete(usize),
    ProtocolErr(Header),
    Success(T),
}

macro_rules! try_outcome {
    ($src:expr) => {
        match $src {
            Outcome::Incomplete(n) => return Outcome::Incomplete(n),
            Outcome::ProtocolErr(header) return Outcome::ProtocolErr(header),
            Outcome::Success(value) => value,
        }
    };
}

use Outcome::{Incomplete, ProtocolErr, Success};

impl Header {
    #[inline]
    fn return_err<T>(self, kind: ErrorKind) -> Outcome<T> {
        Outcome::ProtocolErr(Header::new_error(kind, self.channel(), self.id()))
    }
}

impl<const N: usize> State<N> {
    fn process_data(&mut self, mut buffer: BytesMut) -> Outcome<CompletedRead> {
        // First, attempt to complete a frame.
        loop {
            // We do not have enough data to extract a header, indicate and return.
            if buffer.len() < Header::SIZE {
                return Incomplete(Header::SIZE - buffer.len());
            }

            let header_raw: [u8; Header::SIZE] = buffer[0..Header::SIZE].try_into().unwrap();
            let header = match Header::parse(header_raw) {
                Some(header) => header,
                None => {
                    // The header was invalid, return an error.
                    return ProtocolErr(Header::new_error(
                        ErrorKind::InvalidHeader,
                        UNKNOWN_CHANNEL,
                        UNKNOWN_ID,
                    ));
                }
            };

            // We have a valid header, check if it is an error.
            if header.is_error() {
                // TODO: Read the payload of `OTHER` errors.
                return Success(CompletedRead::ErrorReceived(header));
            }

            // At this point we are guaranteed a valid non-error frame, which has to be on a valid
            // channel.
            let channel = match self.channels.get_mut(header.channel().get() as usize) {
                Some(channel) => channel,
                None => return header.return_err(ErrorKind::InvalidChannel),
            };

            match header.kind() {
                Kind::Request => {
                    if channel.is_at_max_requests() {
                        return header.return_err(ErrorKind::RequestLimitExceeded);
                    }

                    if channel.incoming_requests.insert(header.id()) {
                        return header.return_err(ErrorKind::DuplicateRequest);
                    }

                    // At this point, we have a valid request and its ID has been added to our
                    // incoming set. All we need to do now is to remove it from the buffer.
                    buffer.advance(Header::SIZE);

                    return Success(CompletedRead::NewRequest {
                        id: header.id(),
                        payload: None,
                    });
                }
                Kind::Response => todo!(),
                Kind::RequestPl => match channel.current_request_state {
                    RequestState::Ready => {
                        if channel.is_at_max_requests() {
                            return header.return_err(ErrorKind::RequestLimitExceeded);
                        }

                        if channel.incoming_requests.insert(header.id()) {
                            return header.return_err(ErrorKind::DuplicateRequest);
                        }

                        let segment_buf = &buffer[0..Header::SIZE];

                        match decode_varint32(segment_buf) {
                            Varint32Result::Incomplete => return Incomplete(1),
                            Varint32Result::Overflow => {
                                return header.return_err(ErrorKind::BadVarInt)
                            }
                            Varint32Result::Valid { offset, value } => {
                                // TODO: Check frame boundary.

                                let offset = offset.get() as usize;
                                let total_size = value as usize;

                                let payload_buf = &segment_buf[offset..];
                                if payload_buf.len() >= total_size as usize {
                                    // Entire payload is already in segment. We can just remove it
                                    // from the buffer and return.

                                    buffer.advance(Header::SIZE + offset);
                                    let payload = buffer.split_to(total_size).freeze();
                                    return Success(CompletedRead::NewRequest {
                                        id: header.id(),
                                        payload: Some(payload),
                                    });
                                }

                                todo!() // doesn't fit - check if the segment was filled completely.
                            }
                        }
                    }
                    RequestState::InProgress {
                        header,
                        ref mut payload,
                    } => {
                        todo!()
                    }
                },
                Kind::ResponsePl => todo!(),
                Kind::CancelReq => todo!(),
                Kind::CancelResp => todo!(),
            }
        }
    }
}
