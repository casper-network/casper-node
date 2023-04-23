mod error;
mod header;

pub use error::Error;
use header::{Header, HeaderFlags, HEADER_SIZE};
use std::{collections::BTreeSet, fmt::Debug};

type ChannelId = u8;
type RequestId = u16;

pub enum ReceiveOutcome<'a> {
    /// We need at least the given amount of additional bytes before another item is produced.
    NeedMore(usize),
    Consumed {
        channel: u8,
        raw_message: Frame<'a>,
        bytes_consumed: usize,
    },
}

pub enum Frame<'a> {
    Request {
        id: RequestId,
        payload: Option<&'a [u8]>,
    },
    Response {
        id: RequestId,
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
    pub fn input<'a>(&mut self, buf: &'a [u8]) -> Result<ReceiveOutcome<'a>, Error> {
        let header_raw = match <[u8; HEADER_SIZE]>::try_from(&buf[0..HEADER_SIZE]) {
            Ok(v) => v,
            Err(_) => return Ok(ReceiveOutcome::NeedMore(HEADER_SIZE - buf.len())),
        };

        let header = Header::try_from(header_raw).map_err(Error::InvalidFlags)?;

        let start = buf.as_ptr() as usize;
        let no_header_buf = &buf[HEADER_SIZE..];

        // Process a new header:
        match header.flags {
            HeaderFlags::ZeroSizedRequest => todo!(),
            HeaderFlags::ZeroSizedResponse => todo!(),
            HeaderFlags::Error => todo!(),
            HeaderFlags::RequestCancellation => todo!(),
            HeaderFlags::ResponseCancellation => todo!(),
            HeaderFlags::RequestWithPayload => {
                let channel_id = self.validate_request(&header)?;

                match self.read_variable_payload(no_header_buf) {
                    Ok(payload) => {
                        self.channel_mut(channel_id).pending.insert(header.id);

                        Ok(ReceiveOutcome::Consumed {
                            channel: header.channel,
                            raw_message: Frame::Request {
                                id: header.id,
                                payload: Some(payload),
                            },
                            bytes_consumed: payload.as_ptr() as usize - start + payload.len(),
                        })
                    }
                    Err(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ResponseWithPayload => {
                let channel_id = self.validate_response(&header)?;

                match self.read_variable_payload(no_header_buf) {
                    Ok(payload) => {
                        self.channel_mut(channel_id).pending.remove(&header.id);

                        Ok(ReceiveOutcome::Consumed {
                            channel: header.channel,
                            raw_message: Frame::Request {
                                id: header.id,
                                payload: Some(payload),
                            },
                            bytes_consumed: payload.as_ptr() as usize - start + payload.len(),
                        })
                    }
                    Err(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ErrorWithMessage => todo!(),
        }
    }

    fn read_variable_payload<'a>(&self, buf: &'a [u8]) -> Result<&'a [u8], usize> {
        let Some((payload_len, consumed)) = read_varint_u32(buf)
        else {
            return Err(1);
        };

        let payload_len = payload_len as usize;

        // TODO: Limit max payload length.

        let fragment = &buf[consumed..];
        if fragment.len() < payload_len {
            return Err(payload_len - fragment.len());
        }
        let payload = &fragment[..payload_len];
        Ok(payload)
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
}

fn read_varint_u32(input: &[u8]) -> Option<(u32, usize)> {
    // TODO: Handle overflow (should be an error)?

    let mut num = 0u32;

    for (idx, &c) in input.iter().enumerate() {
        num |= (c & 0b0111_1111) as u32;

        if c & 0b1000_0000 != 0 {
            // More bits will follow.
            num <<= 7;
        } else {
            return Some((num, idx + 1));
        }
    }

    // We found no stop bit, so our integer is incomplete.
    None
}
