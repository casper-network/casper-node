use std::{fmt::Debug, mem};

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
    flags: u8,
}

impl Header {
    #[inline(always)]
    fn is_request(&self) -> bool {
        todo!()
    }
}

impl From<[u8; 4]> for Header {
    fn from(value: [u8; 4]) -> Self {
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        Header {
            id: u16::from_le_bytes(value[2..4].try_into().unwrap()),
            channel: value[1],
            flags: value[0],
        }
    }
}

impl From<Header> for [u8; 4] {
    #[inline(always)]
    fn from(header: Header) -> Self {
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        [
            header.flags,
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
                self.current_header
                    .insert(Header::from(buf.get_u32_le().to_le_bytes()))
            }
            Some(ref header) => header,
        };

        todo!()
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
