use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use bytes::Buf;

type ChannelId = u8;
type RequestId = u16;

const HEADER_SIZE: usize = 4;

enum ReceiveOutcome {
    /// We need at least the given amount of additional bytes before another item is produced.
    MissingAtLeast(usize),
}

#[derive(Debug)]
struct Receiver<const N: usize> {
    current_header: Option<Header>,
    payload_length: Option<usize>,
    channels: [Channel; N],
    request_limits: [usize; N],
    segment_limit: u32,
}

#[derive(Debug)]
struct Channel {
    pending_requests: BTreeSet<RequestId>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C, packed)]
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
    fn input<B: Buf>(&mut self, buf: &mut B) -> ReceiveOutcome {
        let header = match self.current_header {
            None => {
                // Check if we have enough to read a header.
                if buf.remaining() < HEADER_SIZE {
                    return ReceiveOutcome::MissingAtLeast(HEADER_SIZE - buf.remaining());
                }

                // Grab the header and advance.
                let header = Header::try_from(buf.get_u32_le().to_le_bytes())
                    .expect("TODO: add error handling, invalid error");

                // Process a new header:
                match header.flags {
                    HeaderFlags::RequestWithPayload => {
                        let channel_id = if (header.channel as usize) < N {
                            header.channel as usize
                        } else {
                            panic!("TODO: handle error (invalid channel)");
                        };
                        let channel = &mut self.channels[channel_id];
                        let request_id = header.id;

                        if channel.pending_requests.len() >= self.request_limits[channel_id] {
                            panic!("TODO: handle too many requests");
                        }

                        if channel.pending_requests.contains(&request_id) {
                            panic!("TODO: handle duplicate request");
                        }

                        // Now we know that we have received a valid new request, continue to
                        // process data as normal.
                    }
                    HeaderFlags::ResponseWithPayload => todo!(),
                    HeaderFlags::Error => todo!(),
                    HeaderFlags::ErrorWithMessage => todo!(),
                    HeaderFlags::RequestCancellation => todo!(),
                    HeaderFlags::ResponseCancellation => todo!(),
                    HeaderFlags::ZeroSizedRequest => todo!(),
                    HeaderFlags::ZeroSizedResponse => todo!(),
                }

                self.current_header.insert(header)
            }
            Some(ref header) => header,
        };

        match header.flags {
            HeaderFlags::ZeroSizedRequest => todo!(),
            HeaderFlags::ZeroSizedResponse => todo!(),
            HeaderFlags::Error => todo!(),
            HeaderFlags::RequestCancellation => todo!(),
            HeaderFlags::ResponseCancellation => todo!(),
            HeaderFlags::RequestWithPayload => {
                if let Some(len, consumed) = read_varint()
            }
            HeaderFlags::ResponseWithPayload => todo!(),
            HeaderFlags::ErrorWithMessage => todo!(),
        }

        todo!();
    }
}

fn read_varint(input: &[u8]) -> Option<(u32, usize)> {
    let mut num = 0u32;

    for (idx, &c) in input.iter().enumerate() {
        num |= (c & 0b0111_1111) as u32;

        if c & 0b1000_0000 != 0 {
            // More to follow.
            num <<= 7;
        } else {
            return Some((num, idx + 1));
        }
    }

    // We found no stop condition, so our integer is incomplete.
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

        assert_eq!(Header::try_from(input).unwrap(), expected);
        assert_eq!(<[u8; 4]>::from(expected), input);
    }
}
