//! `juliet` header parsing and serialization.
//!
//! This module is typically only used by the protocol implementation (see
//! [`protocol`](crate::protocol)), but may be of interested to those writing low level tooling.
use std::fmt::Debug;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;

use crate::{ChannelId, Id};

/// Header structure.
///
/// Implements [`AsRef<u8>`], which will return a byte slice with the correct encoding of the header
/// that can be sent directly to a peer.
#[derive(Copy, Clone, Eq, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct Header([u8; Header::SIZE]);

impl Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_error() {
            write!(
                f,
                "[err:{:?} chan: {} id: {}]",
                self.error_kind(),
                self.channel(),
                self.id()
            )
        } else {
            write!(
                f,
                "[{:?} chan: {} id: {}]",
                self.kind(),
                self.channel(),
                self.id()
            )
        }
    }
}

/// Error kind, from the kind byte.
#[derive(Copy, Clone, Debug, Error, Eq, PartialEq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
#[repr(u8)]
pub enum ErrorKind {
    /// Application defined error.
    #[error("application defined error")]
    Other = 0,
    /// The maximum frame size has been exceeded. This error cannot occur in this implementation,
    /// which operates solely on streams.
    #[error("maximum frame size exceeded")]
    MaxFrameSizeExceeded = 1,
    /// An invalid header was received.
    #[error("invalid header")]
    InvalidHeader = 2,
    /// A segment was sent with a frame where none was allowed, or a segment was too small or
    /// missing.
    #[error("segment violation")]
    SegmentViolation = 3,
    /// A `varint32` could not be decoded.
    #[error("bad varint")]
    BadVarInt = 4,
    /// Invalid channel: A channel number greater than the highest channel number was received.
    #[error("invalid channel")]
    InvalidChannel = 5,
    /// A new request or response was sent without completing the previous one.
    #[error("multi-frame in progress")]
    InProgress = 6,
    /// The indicated size of the response would exceed the configured limit.
    #[error("response too large")]
    ResponseTooLarge = 7,
    /// The indicated size of the request would exceed the configured limit.
    #[error("request too large")]
    RequestTooLarge = 8,
    /// Peer attempted to create two in-flight requests with the same ID on the same channel.
    #[error("duplicate request")]
    DuplicateRequest = 9,
    /// Sent a response for request not in-flight.
    #[error("response for fictitious request")]
    FictitiousRequest = 10,
    /// The dynamic request limit has been exceeded.
    #[error("request limit exceeded")]
    RequestLimitExceeded = 11,
    /// Response cancellation for a request not in-flight.
    #[error("cancellation for fictitious request")]
    FictitiousCancel = 12,
    /// Peer sent a request cancellation exceeding the cancellation allowance.
    #[error("cancellation limit exceeded")]
    CancellationLimitExceeded = 13,
    // Note: When adding additional kinds, update the `HIGHEST` associated constant.
}

/// Frame kind, from the kind byte.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
#[repr(u8)]

pub enum Kind {
    /// A request with no payload.
    Request = 0,
    /// A response with no payload.
    Response = 1,
    /// A request that includes a payload.
    RequestPl = 2,
    /// A response that includes a payload.
    ResponsePl = 3,
    /// Cancellation of a request.
    CancelReq = 4,
    /// Cancellation of a response.
    CancelResp = 5,
    // Note: When adding additional kinds, update the `HIGHEST` associated constant.
}

impl ErrorKind {
    /// The highest error kind number.
    ///
    /// Only error kinds <= `HIGHEST` are valid.
    const HIGHEST: Self = Self::CancellationLimitExceeded;
}

impl Kind {
    /// The highest frame kind number.
    ///
    /// Only error kinds <= `HIGHEST` are valid.
    const HIGHEST: Self = Self::CancelResp;
}

impl Header {
    /// The size (in bytes) of a header.
    pub(crate) const SIZE: usize = 4;
    /// Bitmask returning the error bit of the kind byte.
    const KIND_ERR_BIT: u8 = 0b1000_0000;
    /// Bitmask returning the error kind inside the kind byte.
    const KIND_ERR_MASK: u8 = 0b0000_1111;
    /// Bitmask returning the frame kind inside the kind byte.
    const KIND_MASK: u8 = 0b0000_0111;

    /// Creates a new non-error header.
    #[inline(always)]
    pub const fn new(kind: Kind, channel: ChannelId, id: Id) -> Self {
        let id = id.get().to_le_bytes();
        Header([kind as u8, channel.get(), id[0], id[1]])
    }

    /// Creates a new error header.
    #[inline(always)]
    pub const fn new_error(kind: ErrorKind, channel: ChannelId, id: Id) -> Self {
        let id = id.get().to_le_bytes();
        Header([
            kind as u8 | Header::KIND_ERR_BIT,
            channel.get(),
            id[0],
            id[1],
        ])
    }

    /// Parse a header from raw bytes.
    ///
    /// Returns `None` if the given `raw` bytes are not a valid header.
    #[inline(always)]
    pub const fn parse(mut raw: [u8; Header::SIZE]) -> Option<Self> {
        // Zero-out reserved bits.
        raw[0] &= Self::KIND_ERR_MASK | Self::KIND_MASK | Self::KIND_ERR_BIT;

        let header = Header(raw);

        // Check that the kind byte is within valid range.
        if header.is_error() {
            if (header.kind_byte() & Self::KIND_ERR_MASK) > ErrorKind::HIGHEST as u8 {
                return None;
            }
        } else {
            if (header.kind_byte() & Self::KIND_MASK) > Kind::HIGHEST as u8 {
                return None;
            }

            // Ensure the 4th bit is not set, since the error kind bits are superset of kind bits.
            if header.0[0] & Self::KIND_MASK != header.0[0] {
                return None;
            }
        }

        Some(header)
    }

    /// Returns the raw kind byte.
    #[inline(always)]
    const fn kind_byte(self) -> u8 {
        self.0[0]
    }

    /// Returns the channel.
    #[inline(always)]
    pub const fn channel(self) -> ChannelId {
        ChannelId::new(self.0[1])
    }

    /// Returns the id.
    #[inline(always)]
    pub const fn id(self) -> Id {
        let [_, _, id @ ..] = self.0;
        Id::new(u16::from_le_bytes(id))
    }

    /// Returns whether the error bit is set.
    #[inline(always)]
    pub const fn is_error(self) -> bool {
        self.kind_byte() & Self::KIND_ERR_BIT == Self::KIND_ERR_BIT
    }

    /// Returns whether or not the given header is a request header.
    #[inline]
    pub const fn is_request(self) -> bool {
        if !self.is_error() {
            matches!(self.kind(), Kind::Request | Kind::RequestPl)
        } else {
            false
        }
    }

    /// Returns the error kind.
    ///
    /// # Panics
    ///
    /// Will panic if `Self::is_error()` is not `true`.
    #[inline(always)]
    pub const fn error_kind(self) -> ErrorKind {
        debug_assert!(self.is_error());
        match self.kind_byte() & Self::KIND_ERR_MASK {
            0 => ErrorKind::Other,
            1 => ErrorKind::MaxFrameSizeExceeded,
            2 => ErrorKind::InvalidHeader,
            3 => ErrorKind::SegmentViolation,
            4 => ErrorKind::BadVarInt,
            5 => ErrorKind::InvalidChannel,
            6 => ErrorKind::InProgress,
            7 => ErrorKind::ResponseTooLarge,
            8 => ErrorKind::RequestTooLarge,
            9 => ErrorKind::DuplicateRequest,
            10 => ErrorKind::FictitiousRequest,
            11 => ErrorKind::RequestLimitExceeded,
            12 => ErrorKind::FictitiousCancel,
            13 => ErrorKind::CancellationLimitExceeded,
            // Would violate validity invariant.
            _ => unreachable!(),
        }
    }

    /// Returns the frame kind.
    ///
    /// # Panics
    ///
    /// Will panic if `Self::is_error()` is `true`.
    #[inline(always)]
    pub const fn kind(self) -> Kind {
        debug_assert!(!self.is_error());
        match self.kind_byte() & Self::KIND_MASK {
            0 => Kind::Request,
            1 => Kind::Response,
            2 => Kind::RequestPl,
            3 => Kind::ResponsePl,
            4 => Kind::CancelReq,
            5 => Kind::CancelResp,
            // Would violate validity invariant.
            _ => unreachable!(),
        }
    }

    /// Creates a new header with the same id and channel but an error kind.
    #[inline]
    pub(crate) const fn with_err(self, kind: ErrorKind) -> Self {
        Header::new_error(kind, self.channel(), self.id())
    }
}

impl From<Header> for [u8; Header::SIZE] {
    fn from(value: Header) -> Self {
        value.0
    }
}

impl AsRef<[u8; Header::SIZE]> for Header {
    fn as_ref(&self) -> &[u8; Header::SIZE] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use bytemuck::Zeroable;
    use proptest::{
        arbitrary::any,
        prelude::Arbitrary,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };
    use proptest_attr_macro::proptest;

    use crate::{ChannelId, Id};

    use super::{ErrorKind, Header, Kind};

    /// Proptest strategy for `Header`s.
    fn arb_header() -> impl Strategy<Value = Header> {
        prop_oneof![
            any::<(Kind, ChannelId, Id)>().prop_map(|(kind, chan, id)| Header::new(kind, chan, id)),
            any::<(ErrorKind, ChannelId, Id)>()
                .prop_map(|(err_kind, chan, id)| Header::new_error(err_kind, chan, id)),
        ]
    }

    impl Arbitrary for Header {
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            arb_header().boxed()
        }

        type Strategy = BoxedStrategy<Header>;
    }

    #[test]
    fn known_headers() {
        let input = [0x86, 0x48, 0xAA, 0xBB];
        let expected =
            Header::new_error(ErrorKind::InProgress, ChannelId::new(0x48), Id::new(0xBBAA));

        assert_eq!(
            Header::parse(input).expect("could not parse header"),
            expected
        );
        assert_eq!(<[u8; Header::SIZE]>::from(expected), input);
    }

    #[proptest]
    fn roundtrip_valid_headers(header: Header) {
        let raw: [u8; Header::SIZE] = header.into();

        assert_eq!(
            Header::parse(raw).expect("failed to roundtrip header"),
            header
        );

        // Verify the `kind` and `err_kind` methods don't panic.
        if header.is_error() {
            header.error_kind();
        } else {
            header.kind();
        }

        // Verify `is_request` does not panic.
        header.is_request();

        // Ensure `is_request` returns the correct value.
        if !header.is_error() {
            if matches!(header.kind(), Kind::Request) || matches!(header.kind(), Kind::RequestPl) {
                assert!(header.is_request());
            } else {
                assert!(!header.is_request());
            }
        }
    }

    #[proptest]
    fn fuzz_header(raw: [u8; Header::SIZE]) {
        if let Some(header) = Header::parse(raw) {
            let rebuilt = if header.is_error() {
                Header::new_error(header.error_kind(), header.channel(), header.id())
            } else {
                Header::new(header.kind(), header.channel(), header.id())
            };

            // Ensure reserved bits are zeroed upon reading.
            let reencoded: [u8; Header::SIZE] = rebuilt.into();
            assert_eq!(rebuilt, header);
            assert_eq!(reencoded, <[u8; Header::SIZE]>::from(header));

            // Ensure debug doesn't panic.
            assert_eq!(format!("{:?}", header), format!("{:?}", header));

            // Check bytewise it is the same.
            assert_eq!(&reencoded[..], header.as_ref());
        }

        // Otherwise all good, simply failed to parse.
    }

    #[test]
    fn fuzz_header_regressions() {
        // Bit 4, which is not `RESERVED`, but only valid for errors.
        let raw = [8, 0, 0, 0];
        assert!(Header::parse(raw).is_none());

        // Two reserved bits set.
        let raw = [48, 0, 0, 0];
        assert!(Header::parse(raw).is_some());
    }

    #[test]
    fn header_parsing_fails_if_kind_out_of_range() {
        let invalid_err_header = [0b1000_1111, 00, 00, 00];
        assert_eq!(Header::parse(invalid_err_header), None);

        let invalid_ok_header = [0b0000_0111, 00, 00, 00];
        assert_eq!(Header::parse(invalid_ok_header), None);
    }

    #[test]
    fn ensure_zeroed_header_works() {
        assert_eq!(
            Header::zeroed(),
            Header::new(Kind::Request, ChannelId(0), Id(0))
        )
    }

    #[proptest]
    fn err_header_construction(header: Header, error_kind: ErrorKind) {
        let combined = header.with_err(error_kind);

        assert_eq!(header.channel(), combined.channel());
        assert_eq!(header.id(), combined.id());
        assert!(combined.is_error());
        assert_eq!(combined.error_kind(), error_kind);
    }
}
