use std::fmt::Debug;

use bytes::Buf;

const HEADER_SIZE: usize = 4;

enum ReceiveOutcome {
    MissingAtLeast(usize),
}

struct Receiver {
    current_header: Option<Header>,
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
    Request = 0b00000000,
    Response = 0b00000001,
    Error = 0b00000010,
    ErrorWithMessage = 0b00001010,
    RequestCancellation = 0b00000100,
    ResponseCancellation = 0b00000101,
    ZeroSizedRequest = 0b00001000,
    ZeroSizedResponse = 0b00001001,
}

impl TryFrom<u8> for HeaderFlags {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, u8> {
        match value {
            0b00000000 => Ok(HeaderFlags::Request),
            0b00000001 => Ok(HeaderFlags::Response),
            0b00000010 => Ok(HeaderFlags::Error),
            0b00001010 => Ok(HeaderFlags::ErrorWithMessage),
            0b00000100 => Ok(HeaderFlags::RequestCancellation),
            0b00000101 => Ok(HeaderFlags::ResponseCancellation),
            0b00001000 => Ok(HeaderFlags::ZeroSizedRequest),
            0b00001001 => Ok(HeaderFlags::ZeroSizedResponse),
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

impl Receiver {
    fn input<B: Buf>(&mut self, buf: &mut B) -> ReceiveOutcome {
        let header = match self.current_header {
            None => {
                // Check if we have enough to read a header.
                if buf.remaining() < HEADER_SIZE {
                    return ReceiveOutcome::MissingAtLeast(HEADER_SIZE - buf.remaining());
                }

                // Grab the header and continue.
                self.current_header.insert(
                    Header::try_from(buf.get_u32_le().to_le_bytes())
                        .expect("TODO: add error handling"),
                )
            }
            Some(ref header) => header,
        };

        match header.flags {
            HeaderFlags::Request => todo!(),
            HeaderFlags::Response => todo!(),
            HeaderFlags::Error => todo!(),
            HeaderFlags::ErrorWithMessage => todo!(),
            HeaderFlags::RequestCancellation => todo!(),
            HeaderFlags::ResponseCancellation => todo!(),
            HeaderFlags::ZeroSizedRequest => todo!(),
            HeaderFlags::ZeroSizedResponse => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Header;

    #[test]
    fn known_headers() {
        let input = [0x12, 0x34, 0x56, 0x78];
        let expected = Header {
            flags: 0x12,   // 18
            channel: 0x34, // 52
            id: 0x7856,    // 30806
        };

        assert_eq!(Header::from(input), expected);
        assert_eq!(<[u8; 4]>::from(expected), input);
    }
}
