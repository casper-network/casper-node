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
        raw_message: RawMessage<'a>,
        bytes_consumed: usize,
    },
}

pub enum RawMessage<'a> {
    NewRequest { id: u16, payload: Option<&'a [u8]> },
}

#[derive(Debug)]
pub struct Receiver<const N: usize> {
    channels: [Channel; N],
    request_limits: [usize; N],
    frame_size_limit: u32,
}

#[derive(Debug)]
struct Channel {
    pending_requests: BTreeSet<RequestId>,
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
                let channel_id = if (header.channel as usize) < N {
                    header.channel as usize
                } else {
                    return Err(Error::InvalidChannel(header.channel));
                };
                let channel = &mut self.channels[channel_id];

                if channel.pending_requests.len() >= self.request_limits[channel_id] {
                    return Err(Error::RequestLimitExceeded);
                }

                if channel.pending_requests.contains(&header.id) {
                    return Err(Error::DuplicateRequest);
                }

                match self.read_variable_payload(no_header_buf) {
                    Ok(payload) => Ok(ReceiveOutcome::Consumed {
                        channel: header.channel,
                        raw_message: RawMessage::NewRequest {
                            id: header.id,
                            payload: Some(payload),
                        },
                        bytes_consumed: payload.as_ptr() as usize - start + payload.len(),
                    }),
                    Err(needed) => Ok(ReceiveOutcome::NeedMore(needed)),
                }
            }
            HeaderFlags::ResponseWithPayload => todo!(),
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
