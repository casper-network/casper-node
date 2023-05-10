mod error;
mod header;

pub use error::Error;
use header::{Header, HeaderFlags, HEADER_SIZE};
use std::{collections::BTreeSet, fmt::Debug};

type ChannelId = u8; // TODO: newtype
type RequestId = u16; // TODO: newtype

pub enum ReceiveOutcome<T> {
    /// We need at least the given amount of additional bytes before another item is produced.
    NeedMore(usize),
    Consumed {
        value: T,
        bytes_consumed: usize,
    },
}

pub enum Frame<'a> {
    Request {
        id: RequestId,
        channel: ChannelId,
        payload: Option<&'a [u8]>,
    },
    Response {
        id: RequestId,
        channel: ChannelId,
        payload: Option<&'a [u8]>,
    },
    Error {
        code: RequestId, // TODO: Use error type here?
        unverified_channel: u8,
        payload: Option<&'a [u8]>,
    },
    RequestCancellation {
        id: RequestId,
        channel: ChannelId,
    },
}

#[derive(Debug)]
pub struct Receiver<const N: usize> {
    channels: [Channel; N],
    request_limits: [u64; N], // TODO: Consider moving to `Channel`, see also: `increase_cancellation_allowance)`.
    frame_size_limit: u32,
}

#[derive(Debug)]
struct Channel {
    pending: BTreeSet<RequestId>,
    cancellation_allowance: u64, // TODO: Upper bound by max request in flight?
}

impl Channel {
    fn increase_cancellation_allowance(&mut self, request_limit: u64) {
        self.cancellation_allowance = (self.cancellation_allowance + 1).min(request_limit);
    }

    fn attempt_cancellation(&mut self) -> bool {
        if self.cancellation_allowance > 0 {
            self.cancellation_allowance -= 1;
            true
        } else {
            false
        }
    }
}

impl<const N: usize> Receiver<N> {
    pub fn input<'a>(&mut self, buf: &'a [u8]) -> Result<ReceiveOutcome<Frame<'a>>, Error> {
        let header_raw = match <[u8; HEADER_SIZE]>::try_from(&buf[0..HEADER_SIZE]) {
            Ok(v) => v,
            Err(_) => return Ok(ReceiveOutcome::NeedMore(HEADER_SIZE - buf.len())),
        };

        let header = Header::try_from(header_raw).map_err(Error::InvalidFlags)?;

        let no_header_buf = &buf[HEADER_SIZE..];

        // Process a new header:
        match header.flags {
            HeaderFlags::ZeroSizedRequest => {
                let channel = self.validate_request(&header)?;
                let request_limit = self.request_limit(channel);
                self.channel_mut(channel)
                    .increase_cancellation_allowance(request_limit);

                let frame = Frame::Request {
                    id: header.id,
                    channel,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: frame,
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::ZeroSizedResponse => {
                let channel = self.validate_response(&header)?;
                let frame = Frame::Response {
                    id: header.id,
                    channel,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: frame,
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::Error => {
                let frame = Frame::Error {
                    code: header.id,
                    unverified_channel: header.channel,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: frame,
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::RequestCancellation => {
                let channel = self.validate_request_cancellation(&header)?;
                let frame = Frame::RequestCancellation {
                    id: header.id,
                    channel,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: frame,
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::ResponseCancellation => {
                // TODO: Find a solution, we need to track requests without race conditions here.
                todo!()
            }
            HeaderFlags::RequestWithPayload => {
                let channel = self.validate_request(&header)?;

                match read_variable_payload(no_header_buf, self.segment_size_limit())? {
                    ReceiveOutcome::Consumed {
                        value,
                        mut bytes_consumed,
                    } => {
                        bytes_consumed += HEADER_SIZE;
                        self.channel_mut(channel).pending.insert(header.id);
                        let request_limit = self.request_limit(channel);
                        self.channel_mut(channel)
                            .increase_cancellation_allowance(request_limit);

                        let frame = Frame::Request {
                            id: header.id,
                            channel,
                            payload: Some(value),
                        };

                        Ok(ReceiveOutcome::Consumed {
                            value: frame,
                            bytes_consumed,
                        })
                    }
                    ReceiveOutcome::NeedMore(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ResponseWithPayload => {
                let channel = self.validate_response(&header)?;

                match read_variable_payload(no_header_buf, self.segment_size_limit())? {
                    ReceiveOutcome::Consumed {
                        value,
                        mut bytes_consumed,
                    } => {
                        bytes_consumed += HEADER_SIZE;
                        self.channel_mut(channel).pending.remove(&header.id);

                        let frame = Frame::Request {
                            id: header.id,
                            channel,
                            payload: Some(value),
                        };

                        Ok(ReceiveOutcome::Consumed {
                            value: frame,
                            bytes_consumed,
                        })
                    }
                    ReceiveOutcome::NeedMore(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ErrorWithMessage => {
                match read_variable_payload(no_header_buf, self.segment_size_limit())? {
                    ReceiveOutcome::Consumed {
                        value,
                        mut bytes_consumed,
                    } => {
                        bytes_consumed += HEADER_SIZE;

                        let frame = Frame::Error {
                            code: header.id,
                            unverified_channel: header.channel,
                            payload: Some(value),
                        };

                        Ok(ReceiveOutcome::Consumed {
                            value: frame,
                            bytes_consumed,
                        })
                    }
                    ReceiveOutcome::NeedMore(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
        }
    }

    fn validate_channel(header: &Header) -> Result<ChannelId, Error> {
        if (header.channel as usize) < N {
            Ok(header.channel)
        } else {
            Err(Error::InvalidChannel(header.channel))
        }
    }

    fn validate_request(&self, header: &Header) -> Result<ChannelId, Error> {
        let channel_id = Self::validate_channel(&header)?;
        let channel = self.channel(channel_id);

        if channel.pending.len() as u64 >= self.request_limit(channel_id) {
            return Err(Error::RequestLimitExceeded);
        }

        if channel.pending.contains(&header.id) {
            return Err(Error::DuplicateRequest);
        }

        Ok(channel_id)
    }

    fn validate_request_cancellation(&mut self, header: &Header) -> Result<ChannelId, Error> {
        let channel_id = Self::validate_channel(&header)?;
        let channel = self.channel_mut(channel_id);
        if !channel.attempt_cancellation() {
            Err(Error::ExceededRequestCancellationAllowance)
        } else {
            Ok(channel_id)
        }
    }

    fn validate_response(&self, header: &Header) -> Result<ChannelId, Error> {
        let channel_id = Self::validate_channel(&header)?;
        let channel = self.channel(channel_id);

        if !channel.pending.contains(&header.id) {
            return Err(Error::FicticiousRequest(header.id));
        }

        Ok(channel_id)
    }

    fn channel(&self, channel_id: ChannelId) -> &Channel {
        &self.channels[channel_id as usize]
    }

    fn channel_mut(&mut self, channel_id: ChannelId) -> &mut Channel {
        &mut self.channels[channel_id as usize]
    }

    fn request_limit(&self, channel_id: ChannelId) -> u64 {
        self.request_limits[channel_id as usize]
    }

    fn segment_size_limit(&self) -> usize {
        self.frame_size_limit.saturating_sub(HEADER_SIZE as u32) as usize
    }
}

fn read_varint_u32(input: &[u8]) -> Result<ReceiveOutcome<u32>, Error> {
    // TODO: Handle overflow (should be an error)?

    let mut value = 0u32;

    for (idx, &c) in input.iter().enumerate() {
        value |= (c & 0b0111_1111) as u32;

        if c & 0b1000_0000 != 0 {
            if idx > 5 {
                return Err(Error::VarIntOverflow);
            }

            // More bits will follow.
            value <<= 7;
        } else {
            return Ok(ReceiveOutcome::Consumed {
                value,
                bytes_consumed: idx + 1,
            });
        }
    }

    // We found no stop bit, so our integer is incomplete.
    Ok(ReceiveOutcome::NeedMore(1))
}

fn read_variable_payload<'a>(
    buf: &'a [u8],
    limit: usize,
) -> Result<ReceiveOutcome<&'a [u8]>, Error> {
    let (value_len, mut bytes_consumed) = match read_varint_u32(buf)? {
        ReceiveOutcome::NeedMore(needed) => return Ok(ReceiveOutcome::NeedMore(needed)),
        ReceiveOutcome::Consumed {
            value,
            bytes_consumed,
        } => (value, bytes_consumed),
    };

    let value_len = value_len as usize;

    if value_len + bytes_consumed < limit {
        return Err(Error::SegmentSizedExceeded(value_len + bytes_consumed));
    }

    let payload = &buf[bytes_consumed..];
    if payload.len() < value_len {
        return Ok(ReceiveOutcome::NeedMore(value_len - payload.len()));
    }

    let value = &payload[..value_len];
    bytes_consumed += value.len();
    Ok(ReceiveOutcome::Consumed {
        value,
        bytes_consumed,
    })
}
