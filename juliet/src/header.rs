use crate::{ChannelId, RequestId};

/// `juliet` header parsing and serialization.

/// The size of a header in bytes.
pub(crate) const HEADER_SIZE: usize = 4;

/// Header structure.
///
/// This struct guaranteed to be 1:1 bit compatible to actually serialized headers on little endian
/// machines, thus serialization/deserialization should be no-ops when compiled with optimizations.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub(crate) struct Header {
    /// Request/response ID.
    pub(crate) id: RequestId,
    /// Channel for the frame this header belongs to.
    pub(crate) channel: ChannelId,
    /// Flags.
    ///
    /// See protocol documentation for details.
    pub(crate) flags: HeaderFlags,
}

/// Header flags.
///
/// At the moment, all flag combinations available require separate code-paths for handling anyway,
/// thus there are no true "optional" flags. Thus for simplicity, an `enum` is used at the moment.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum HeaderFlags {
    /// A request without a segment following it.
    ZeroSizedRequest = 0b00000000,
    /// A response without a segment following it.
    ZeroSizedResponse = 0b00000001,
    /// An error with no detail segment.
    Error = 0b00000011,
    /// Cancellation of a request.
    RequestCancellation = 0b00000100,
    /// Cancellation of a response.
    ResponseCancellation = 0b00000101,
    /// A request with a segment following it.
    RequestWithPayload = 0b00001000,
    /// A response with a segment following it.
    ResponseWithPayload = 0b00001001,
    /// An error with a detail segment.
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
    type Error = u8; // Invalid flags are returned as the error.

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
