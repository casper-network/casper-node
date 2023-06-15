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
    current_multiframe_receive: MultiframeSendState,
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
    ReceivedResponse { id: Id, payload: Option<Bytes> },
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
            Outcome::ProtocolErr(header) => return Outcome::ProtocolErr(header),
            Outcome::Success(value) => value,
        }
    };
}

use Outcome::{Incomplete, ProtocolErr, Success};

use self::multiframe::MultiframeSendState;

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
                match header.error_kind() {
                    ErrorKind::Other => {
                        // TODO: `OTHER` errors may contain a payload.

                        unimplemented!()
                    }
                    _ => {
                        return Success(CompletedRead::ErrorReceived(header));
                    }
                }
            }

            // At this point we are guaranteed a valid non-error frame, verify its channel.
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
                Kind::Response => {
                    if !channel.outgoing_requests.remove(&header.id()) {
                        return header.return_err(ErrorKind::FictitiousRequest);
                    } else {
                        return Success(CompletedRead::ReceivedResponse {
                            id: header.id(),
                            payload: None,
                        });
                    }
                }
                Kind::RequestPl => {
                    // First, we need to "gate" the incoming request; it only gets to bypass the request limit if it is already in progress:
                    let is_new_request = channel.current_multiframe_receive.is_new_transfer(header);

                    if is_new_request {
                        // If we're in the ready state, requests must be eagerly rejected if
                        // exceeding the limit.
                        if channel.is_at_max_requests() {
                            return header.return_err(ErrorKind::RequestLimitExceeded);
                        }

                        // We also check for duplicate requests early to avoid reading them.
                        if channel.incoming_requests.contains(&header.id()) {
                            return header.return_err(ErrorKind::DuplicateRequest);
                        }
                    };

                    let multiframe_outcome: Option<BytesMut> = try_outcome!(channel
                        .current_multiframe_receive
                        .accept(header, &mut buffer, self.max_frame_size));

                    // If we made it to this point, we have consumed the frame. Record it.
                    if is_new_request {
                        if channel.incoming_requests.insert(header.id()) {
                            return header.return_err(ErrorKind::DuplicateRequest);
                        }
                    }

                    match multiframe_outcome {
                        Some(payload) => {
                            // Message is complete.
                            return Success(CompletedRead::NewRequest {
                                id: header.id(),
                                payload: Some(payload.freeze()),
                            });
                        }
                        None => {
                            // We need more frames to complete the payload. Do nothing and attempt
                            // to read the next frame.
                        }
                    }
                }
                Kind::ResponsePl => {
                    let is_new_response =
                        channel.current_multiframe_receive.is_new_transfer(header);

                    // Ensure it is not a bogus response.
                    if is_new_response {
                        if !channel.outgoing_requests.contains(&header.id()) {
                            return header.return_err(ErrorKind::FictitiousRequest);
                        }
                    }

                    let multiframe_outcome: Option<BytesMut> = try_outcome!(channel
                        .current_multiframe_receive
                        .accept(header, &mut buffer, self.max_frame_size));

                    // If we made it to this point, we have consumed the frame.
                    if is_new_response {
                        if !channel.outgoing_requests.remove(&header.id()) {
                            return header.return_err(ErrorKind::FictitiousRequest);
                        }
                    }

                    match multiframe_outcome {
                        Some(payload) => {
                            // Message is complete.
                            return Success(CompletedRead::ReceivedResponse {
                                id: header.id(),
                                payload: Some(payload.freeze()),
                            });
                        }
                        None => {
                            // We need more frames to complete the payload. Do nothing and attempt
                            // to read the next frame.
                        }
                    }
                }
                Kind::CancelReq => todo!(),
                Kind::CancelResp => todo!(),
            }
        }
    }
}
