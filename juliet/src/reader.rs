mod multiframe;

use std::{collections::HashSet, marker::PhantomData, mem, ops::Deref};

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

pub(crate) enum Outcome<T> {
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

use self::multiframe::RequestState;

impl Header {
    #[inline]
    pub(crate) fn return_err<T>(self, kind: ErrorKind) -> Outcome<T> {
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
                        total_payload_size,
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
