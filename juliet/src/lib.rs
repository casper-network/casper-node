mod error;

pub use error::Error;
use std::{collections::BTreeSet, fmt::Debug};

type ChannelId = u8;
type RequestId = u16;

const HEADER_SIZE: usize = 4;

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)] // TODO: See if we need `packed` or not. Maybe add a test?
struct Header {
    id: u16,
    channel: u8,
    flags: HeaderFlags,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
enum HeaderFlags {
    ZeroSizedRequest = 0b00000000,
    ZeroSizedResponse = 0b00000001,
    Error = 0b00000011,
    RequestCancellation = 0b00000100,
    ResponseCancellation = 0b00000101,
    RequestWithPayload = 0b00001000,
    ResponseWithPayload = 0b00001001,
    ErrorWithMessage = 0b00001010,
}

impl TryFrom<u8> for HeaderFlags {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, u8> {
        match value {
            0b00000000 => Ok(HeaderFlags::ZeroSizedRequest),
            0b00000001 => Ok(HeaderFlags::ZeroSizedResponse),
            0b00000011 => Ok(HeaderFlags::Error),
            0b00000100 => Ok(HeaderFlags::RequestCancellation),
            0b00000101 => Ok(HeaderFlags::ResponseCancellation),
            0b00001000 => Ok(HeaderFlags::RequestWithPayload),
            0b00001001 => Ok(HeaderFlags::ResponseWithPayload),
            0b00001010 => Ok(HeaderFlags::ErrorWithMessage),
            _ => Err(value),
        }
    }
}

impl TryFrom<[u8; 4]> for Header {
    type Error = u8; // Invalid flags.

    fn try_from(value: [u8; 4]) -> Result<Self, Self::Error> {
        let flags = HeaderFlags::try_from(value[0])?;
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        Ok(Header {
            // Safe unwrap here, as the size of `value[2..4]` is exactly the necessary 2 bytes.
            id: u16::from_le_bytes(value[2..4].try_into().unwrap()),
            channel: value[1],
            flags,
        })
    }
}

impl From<Header> for [u8; 4] {
    #[inline(always)]
    fn from(header: Header) -> Self {
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        [
            header.flags as u8,
            header.channel,
            header.id.to_le_bytes()[0],
            header.id.to_le_bytes()[1],
        ]
    }
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

#[cfg(test)]
mod tests {
    use crate::{Header, HeaderFlags};

    #[test]
    fn known_headers() {
        let input = [0x09, 0x34, 0x56, 0x78];
        let expected = Header {
            flags: HeaderFlags::ResponseWithPayload,
            channel: 0x34, // 52
            id: 0x7856,    // 30806
        };

        assert_eq!(
            Header::try_from(input).expect("could not parse header"),
            expected
        );
        assert_eq!(<[u8; 4]>::from(expected), input);
    }
}
