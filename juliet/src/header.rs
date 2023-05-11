use std::fmt::Debug;

/// `juliet` header parsing and serialization.
use crate::{ChannelId, Id};
/// Header structure.
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct Header([u8; Self::SIZE]);

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

#[derive(Copy, Clone, Debug)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
#[repr(u8)]
enum ErrorKind {
    Other = 0,
    MaxFrameSizeExceeded = 1,
    InvalidHeader = 2,
    SegmentViolation = 3,
    BadVarInt = 4,
    InvalidChannel = 5,
    InProgress = 6,
    ResponseTooLarge = 7,
    RequestTooLarge = 8,
    DuplicateRequest = 9,
    FictitiousRequest = 10,
    RequestLimitExceeded = 11,
    FictitiousCancel = 12,
    CancellationLimitExceeded = 13,
    // Note: When adding additional kinds, update the `HIGHEST` associated constant.
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
#[repr(u8)]

enum Kind {
    Request = 0,
    Response = 1,
    RequestPl = 2,
    ResponsePl = 3,
    CancelReq = 4,
    CancelResp = 5,
}

impl ErrorKind {
    const HIGHEST: Self = Self::CancellationLimitExceeded;
}

impl Kind {
    const HIGHEST: Self = Self::CancelResp;
}

impl Header {
    const SIZE: usize = 4;
    const KIND_ERR_BIT: u8 = 0b1000_0000;
    const KIND_ERR_MASK: u8 = 0b0000_1111;
    const KIND_MASK: u8 = 0b0000_0111;
}

impl Header {
    #[inline(always)]
    fn new(kind: Kind, channel: ChannelId, id: Id) -> Self {
        let id = id.to_le_bytes();
        Header([kind as u8, channel as u8, id[0], id[1]])
    }

    #[inline(always)]
    fn new_error(kind: ErrorKind, channel: ChannelId, id: Id) -> Self {
        let id = id.to_le_bytes();
        Header([
            kind as u8 | Header::KIND_ERR_BIT,
            channel as u8,
            id[0],
            id[1],
        ])
    }

    #[inline(always)]
    fn parse(raw: [u8; Header::SIZE]) -> Option<Self> {
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
        }

        Some(header)
    }

    #[inline(always)]
    fn kind_byte(self) -> u8 {
        self.0[0]
    }

    #[inline(always)]
    fn channel(self) -> ChannelId {
        self.0[1]
    }

    #[inline(always)]
    fn id(self) -> Id {
        let [_, _, id @ ..] = self.0;
        Id::from_le_bytes(id)
    }

    #[inline(always)]
    fn is_error(self) -> bool {
        self.kind_byte() & Self::KIND_ERR_BIT == Self::KIND_ERR_BIT
    }

    #[inline(always)]
    fn error_kind(self) -> ErrorKind {
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

    #[inline(always)]
    fn kind(self) -> Kind {
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
}

impl From<Header> for [u8; Header::SIZE] {
    fn from(value: Header) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
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
        let expected = Header::new_error(ErrorKind::InProgress, 0x48, 0xBBAA);

        assert_eq!(
            Header::parse(input).expect("could not parse header"),
            expected
        );
        assert_eq!(<[u8; 4]>::from(expected), input);
    }

    #[proptest]
    fn roundtrip_header(header: Header) {
        let raw: [u8; 4] = header.into();

        assert_eq!(
            Header::parse(raw).expect("failed to roundtrip header"),
            header
        );

        // Verify the `kind` and `err_kind` methods don't panic.
        if header.is_error() {
            drop(header.error_kind());
        } else {
            drop(header.kind());
        }
    }
}
