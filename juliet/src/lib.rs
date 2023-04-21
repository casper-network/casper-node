use std::fmt::Debug;

use bitflags::bitflags;
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

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct HeaderFlags: u8 {
        const RESPONSE = 0b00000001;
        const ERROR    = 0b00000010;
        const CANCEL   = 0b00000100;
    }
}

impl Header {
    #[inline(always)]
    fn is_response(&self) -> bool {
        self.flags.contains(HeaderFlags::RESPONSE)
    }

    #[inline(always)]
    fn is_error(&self) -> bool {
        self.flags.contains(HeaderFlags::ERROR)
    }

    #[inline(always)]
    fn is_cancellation(&self) -> bool {
        self.flags.contains(HeaderFlags::CANCEL)
    }
}

impl From<[u8; 4]> for Header {
    fn from(value: [u8; 4]) -> Self {
        let flags = HeaderFlags::from_bits_truncate(value[0]);
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        Header {
            id: u16::from_le_bytes(value[2..4].try_into().unwrap()),
            channel: value[1],
            flags,
        }
    }
}

impl From<Header> for [u8; 4] {
    #[inline(always)]
    fn from(header: Header) -> Self {
        // TODO: Check if this code is equal to `mem::transmute` usage on LE platforms.
        [
            header.flags.bits(),
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

        match (*header).flags {
            flags if flags.is_empty() => {
                // A regular request.
                todo!()
            }
            flags if flags == HeaderFlags::RESPONSE => {
                // A regular response being sent back.
                todo!()
            }
            flags if flags == HeaderFlags::CANCEL => {
                // Request cancellcation.
            }
            flags if flags == HeaderFlags::CANCEL | HeaderFlags::RESPONSE => {
                // Response cancellcation.
            }
            flags if flags == HeaderFlags::ERROR => {
                // Error.
            }
            flags => {
                todo!("invalid flags error")
            }
        }

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
