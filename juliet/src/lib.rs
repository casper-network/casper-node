mod error;
mod header;

pub use error::Error;
use header::{Header, HeaderFlags, HEADER_SIZE};
use std::{collections::BTreeSet, fmt::Debug};

type ChannelId = u8;
type RequestId = u16;

pub enum ReceiveOutcome<T> {
    /// We need at least the given amount of additional bytes before another item is produced.
    NeedMore(usize),
    Consumed {
        value: T,
        bytes_consumed: usize,
    },
}

pub struct Frame<'a> {
    pub channel: ChannelId,
    pub kind: FrameKind<'a>,
}

pub enum FrameKind<'a> {
    Request {
        id: RequestId,
        payload: Option<&'a [u8]>,
    },
    Response {
        id: RequestId,
        payload: Option<&'a [u8]>,
    },
    Error {
        code: RequestId, // TODO: Use error type here?
        payload: Option<&'a [u8]>,
    },
}

#[derive(Debug)]
pub struct Receiver<const N: usize> {
    channels: [Channel; N],
    request_limits: [usize; N],
    frame_size_limit: u32,
}

#[derive(Debug)]
struct Channel {
    pending: BTreeSet<RequestId>,
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
                let kind = FrameKind::Request {
                    id: header.id,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: Frame { channel, kind },
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::ZeroSizedResponse => {
                let channel = self.validate_response(&header)?;
                let kind = FrameKind::Response {
                    id: header.id,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: Frame { channel, kind },
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::Error => {
                let kind = FrameKind::Error {
                    code: header.id,
                    payload: None,
                };

                Ok(ReceiveOutcome::Consumed {
                    value: Frame {
                        channel: header.channel, // TODO: Ok to be unverified?
                        kind,
                    },
                    bytes_consumed: HEADER_SIZE,
                })
            }
            HeaderFlags::RequestCancellation => todo!(),
            HeaderFlags::ResponseCancellation => todo!(),
            HeaderFlags::RequestWithPayload => {
                let channel = self.validate_request(&header)?;

                match read_variable_payload(no_header_buf, self.segment_size_limit())? {
                    ReceiveOutcome::Consumed {
                        value,
                        mut bytes_consumed,
                    } => {
                        bytes_consumed += HEADER_SIZE;
                        self.channel_mut(channel).pending.insert(header.id);

                        let kind = FrameKind::Request {
                            id: header.id,
                            payload: Some(value),
                        };

                        Ok(ReceiveOutcome::Consumed {
                            value: Frame { channel, kind },
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

                        let kind = FrameKind::Request {
                            id: header.id,
                            payload: Some(value),
                        };

                        Ok(ReceiveOutcome::Consumed {
                            value: Frame { channel, kind },
                            bytes_consumed,
                        })
                    }
                    ReceiveOutcome::NeedMore(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ErrorWithMessage => todo!(),
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

        if channel.pending.len() >= self.request_limit(channel_id) {
            return Err(Error::RequestLimitExceeded);
        }

        if channel.pending.contains(&header.id) {
            return Err(Error::DuplicateRequest);
        }

        Ok(channel_id)
    }

    fn validate_response(&self, header: &Header) -> Result<ChannelId, Error> {
        let channel_id = Self::validate_channel(&header)?;
        let channel = self.channel(channel_id);

        if !channel.pending.contains(&header.id) {
            return Err(Error::FictiveRequest(header.id));
        }

        Ok(channel_id)
    }

    fn channel(&self, channel_id: ChannelId) -> &Channel {
        &self.channels[channel_id as usize]
    }

    fn channel_mut(&mut self, channel_id: ChannelId) -> &mut Channel {
        &mut self.channels[channel_id as usize]
    }

    fn request_limit(&self, channel_id: ChannelId) -> usize {
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
