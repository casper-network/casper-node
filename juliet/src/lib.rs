#![doc(html_root_url = "https://docs.rs/juliet/0.1.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(deny(warnings)))
)]
#![warn(missing_docs, trivial_casts, trivial_numeric_casts)]
#![doc = include_str!("../README.md")]

//!
//!
//! ## General usage
//!
//! This crate is split into three layers, whose usage depends on an application's specific use
//! case. At the very core sits the [`protocol`] module, which is a side-effect-free implementation
//! of the protocol. The caller is responsible for all IO flowing in and out, but it is instructed
//! by the state machine what to do next.
//!
//! If there is no need to roll custom IO, the [`io`] layer provides a complete `tokio`-based
//! solution that operates on [`tokio::io::AsyncRead`] and [`tokio::io::AsyncWrite`]. It handles
//! multiplexing input, output and scheduling, as well as buffering messages using a wait and a
//! ready queue.
//!
//! Most users of the library will likely use the highest level layer, [`rpc`] instead. It sits on
//! top the raw [`io`] layer and wraps all the functionality in safe Rust types, making misuse of
//! the underlying protocol hard, if not impossible.

pub mod header;
pub mod io;
pub mod protocol;
pub mod rpc;
pub(crate) mod util;
pub mod varint;

use std::{
    fmt::{self, Display},
    num::NonZeroU32,
};

/// A channel identifier.
///
/// Newtype wrapper to prevent accidental mixups between regular [`u8`]s and those used as channel
/// IDs. Does not indicate whether or not a channel ID is actually valid, i.e. a channel that
/// exists.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
#[repr(transparent)]
pub struct ChannelId(u8);

impl Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl ChannelId {
    /// Creates a new channel ID.
    #[inline(always)]
    pub const fn new(chan: u8) -> Self {
        ChannelId(chan)
    }

    /// Returns the channel ID as [`u8`].
    #[inline(always)]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<ChannelId> for u8 {
    #[inline(always)]
    fn from(value: ChannelId) -> Self {
        value.get()
    }
}

/// An identifier for a `juliet` message.
///
/// Newtype wrapper to prevent accidental mixups between regular [`u16`]s and those used as IDs.
/// Does not indicate whether or not an ID refers to an existing request.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
#[repr(transparent)]
pub struct Id(u16);

impl Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Id {
    /// Creates a new identifier.
    #[inline(always)]
    pub const fn new(id: u16) -> Self {
        Id(id)
    }

    /// Returns the channel ID as [`u16`].
    #[inline(always)]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl From<Id> for u16 {
    #[inline(always)]
    fn from(value: Id) -> Self {
        value.get()
    }
}

/// The outcome of a parsing operation on a potentially incomplete buffer.
#[derive(Debug, Eq, PartialEq)]
#[must_use]
pub enum Outcome<T, E> {
    /// The given data was incomplete, at least the given amount of additional bytes is needed.
    Incomplete(NonZeroU32),
    /// An fatal error was found in the given input.
    Fatal(E),
    /// The parse was successful and the underlying buffer has been modified to extract `T`.
    Success(T),
}

impl<T, E> Outcome<T, E> {
    /// Expects the outcome, similar to [`std::result::Result::expect`].
    ///
    /// Returns the value of [`Outcome::Success`].
    ///
    /// # Panics
    ///
    /// Will panic if the [`Outcome`] is not [`Outcome::Success`].
    #[inline]
    #[track_caller]
    pub fn expect(self, msg: &str) -> T {
        match self {
            Outcome::Success(value) => value,
            Outcome::Incomplete(_) => panic!("incomplete: {}", msg),
            Outcome::Fatal(_) => panic!("error: {}", msg),
        }
    }

    /// Maps the error of an [`Outcome`].
    #[inline]
    pub fn map_err<E2, F>(self, f: F) -> Outcome<T, E2>
    where
        F: FnOnce(E) -> E2,
    {
        match self {
            Outcome::Incomplete(n) => Outcome::Incomplete(n),
            Outcome::Fatal(err) => Outcome::Fatal(f(err)),
            Outcome::Success(value) => Outcome::Success(value),
        }
    }

    /// Helper function to construct an [`Outcome::Incomplete`].
    #[inline]
    #[track_caller]
    pub fn incomplete(remaining: usize) -> Outcome<T, E> {
        Outcome::Incomplete(
            NonZeroU32::new(u32::try_from(remaining).expect("did not expect large usize"))
                .expect("did not expect 0-byte `Incomplete`"),
        )
    }

    /// Converts an [`Outcome`] into a result, panicking on [`Outcome::Incomplete`].
    ///
    /// This function should never be used outside tests.
    #[cfg(test)]
    #[track_caller]
    pub fn to_result(self) -> Result<T, E> {
        match self {
            Outcome::Incomplete(missing) => {
                panic!(
                    "did not expect incompletion by {} bytes converting to result",
                    missing
                )
            }
            Outcome::Fatal(e) => Err(e),
            Outcome::Success(s) => Ok(s),
        }
    }
}

/// `try!` for [`Outcome`].
///
/// Will pass [`Outcome::Incomplete`] and [`Outcome::Fatal`] upwards, or unwrap the value found in
/// [`Outcome::Success`].
#[macro_export]
macro_rules! try_outcome {
    ($src:expr) => {
        match $src {
            Outcome::Incomplete(n) => return Outcome::Incomplete(n),
            Outcome::Fatal(err) => return Outcome::Fatal(err.into()),
            Outcome::Success(value) => value,
        }
    };
}

/// Channel configuration values that needs to be agreed upon by all clients.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChannelConfiguration {
    /// Maximum number of requests allowed on the channel.
    request_limit: u16,
    /// Maximum size of a request sent across the channel.
    max_request_payload_size: u32,
    /// Maximum size of a response sent across the channel.
    max_response_payload_size: u32,
}

impl Default for ChannelConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelConfiguration {
    /// Creates a new [`ChannelConfiguration`] with default values.
    pub const fn new() -> Self {
        Self {
            request_limit: 1,
            max_request_payload_size: 0,
            max_response_payload_size: 0,
        }
    }

    /// Creates a configuration with the given request limit (default is 1).
    pub const fn with_request_limit(mut self, request_limit: u16) -> ChannelConfiguration {
        self.request_limit = request_limit;
        self
    }

    /// Creates a configuration with the given maximum size for request payloads (default is 0).
    ///
    /// There is nothing magical about payload sizes, a size of 0 allows for payloads that are no
    /// longer than 0 bytes in size. On the protocol level, there is a distinction between a request
    /// with a zero-sized payload and no payload.
    pub const fn with_max_request_payload_size(
        mut self,
        max_request_payload_size: u32,
    ) -> ChannelConfiguration {
        self.max_request_payload_size = max_request_payload_size;
        self
    }

    /// Creates a configuration with the given maximum size for response payloads (default is 0).
    ///
    /// There is nothing magical about payload sizes, a size of 0 allows for payloads that are no
    /// longer than 0 bytes in size. On the protocol level, there is a distinction between a
    /// response with a zero-sized payload and no payload.
    pub const fn with_max_response_payload_size(
        mut self,
        max_response_payload_size: u32,
    ) -> ChannelConfiguration {
        self.max_response_payload_size = max_response_payload_size;
        self
    }
}

#[cfg(test)]
mod tests {
    use proptest::{
        prelude::Arbitrary,
        strategy::{Map, Strategy},
    };
    use proptest_attr_macro::proptest;

    use crate::{ChannelConfiguration, ChannelId, Id, Outcome};

    impl Arbitrary for ChannelId {
        type Parameters = <u8 as Arbitrary>::Parameters;

        #[inline]
        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            <u8 as Arbitrary>::arbitrary_with(args).prop_map(Self::new)
        }

        type Strategy = Map<<u8 as Arbitrary>::Strategy, fn(u8) -> Self>;
    }

    impl Arbitrary for Id {
        type Parameters = <u16 as Arbitrary>::Parameters;

        #[inline]
        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            <u16 as Arbitrary>::arbitrary_with(args).prop_map(Self::new)
        }

        type Strategy = Map<<u16 as Arbitrary>::Strategy, fn(u16) -> Self>;
    }

    #[proptest]
    fn id_type_smoke_tests(raw: u16) {
        let id = Id::new(raw);
        assert_eq!(id.get(), raw);
        assert_eq!(u16::from(id), raw);
        assert_eq!(raw.to_string(), id.to_string());
    }

    #[proptest]
    fn channel_type_smoke_tests(raw: u8) {
        let channel_id = ChannelId::new(raw);
        assert_eq!(channel_id.get(), raw);
        assert_eq!(u8::from(channel_id), raw);
        assert_eq!(raw.to_string(), channel_id.to_string());
    }

    #[test]
    fn outcome_incomplete_works_on_non_zero() {
        assert!(matches!(
            Outcome::<(), ()>::incomplete(1),
            Outcome::Incomplete(_)
        ));

        assert!(matches!(
            Outcome::<(), ()>::incomplete(100),
            Outcome::Incomplete(_)
        ));

        assert!(matches!(
            Outcome::<(), ()>::incomplete(u32::MAX as usize),
            Outcome::Incomplete(_)
        ));
    }

    #[test]
    #[should_panic(expected = "did not expect 0-byte `Incomplete`")]
    fn outcome_incomplete_panics_on_0() {
        let _ = Outcome::<(), ()>::incomplete(0);
    }

    #[test]
    #[should_panic(expected = "did not expect large usize")]
    fn outcome_incomplete_panics_past_u32_max() {
        let _ = Outcome::<(), ()>::incomplete(u32::MAX as usize + 1);
    }

    #[test]
    fn outcome_expect_works_on_success() {
        let outcome: Outcome<u32, ()> = Outcome::Success(12);
        assert_eq!(outcome.expect("should not panic"), 12);
    }

    #[test]
    #[should_panic(expected = "is incomplete")]
    fn outcome_expect_panics_on_incomplete() {
        let outcome: Outcome<u32, ()> = Outcome::incomplete(1);
        outcome.expect("is incomplete");
    }

    #[test]
    #[should_panic(expected = "is fatal")]
    fn outcome_expect_panics_on_fatal() {
        let outcome: Outcome<u32, ()> = Outcome::Fatal(());
        outcome.expect("is fatal");
    }

    #[test]
    fn outcome_map_err_works_correctly() {
        let plus_1 = |x: u8| x as u16 + 1;

        let success = Outcome::Success(1);
        assert_eq!(success.map_err(plus_1), Outcome::Success(1));

        let incomplete = Outcome::<(), u8>::incomplete(1);
        assert_eq!(
            incomplete.map_err(plus_1),
            Outcome::<(), u16>::incomplete(1)
        );

        let fatal = Outcome::Fatal(1);
        assert_eq!(fatal.map_err(plus_1), Outcome::<(), u16>::Fatal(2));
    }

    #[test]
    fn outcome_to_result_works_correctly() {
        let success = Outcome::<_, ()>::Success(1);
        assert_eq!(success.to_result(), Ok(1));

        let fatal = Outcome::<(), _>::Fatal(1);
        assert_eq!(fatal.to_result(), Err(1));
    }

    #[test]
    #[should_panic(expected = "did not expect incompletion by 1 bytes converting to result")]
    fn outcome_to_result_panics_on_incomplete() {
        let _ = Outcome::<(), u8>::incomplete(1).to_result();
    }

    #[test]
    fn try_outcome_works() {
        fn try_outcome_func(input: Outcome<u8, i32>) -> Outcome<u16, i32> {
            let value = try_outcome!(input);
            Outcome::Success(value as u16 + 1)
        }

        assert_eq!(try_outcome_func(Outcome::Success(1)), Outcome::Success(2));
        assert_eq!(
            try_outcome_func(Outcome::incomplete(123)),
            Outcome::incomplete(123)
        );
        assert_eq!(try_outcome_func(Outcome::Fatal(-123)), Outcome::Fatal(-123));
    }

    #[test]
    fn channel_configuration_can_be_built() {
        let mut chan_cfg = ChannelConfiguration::new();
        assert_eq!(chan_cfg, ChannelConfiguration::default());

        chan_cfg = chan_cfg.with_request_limit(123);
        assert_eq!(chan_cfg.request_limit, 123);

        chan_cfg = chan_cfg.with_max_request_payload_size(99);
        assert_eq!(chan_cfg.request_limit, 123);
        assert_eq!(chan_cfg.max_request_payload_size, 99);

        chan_cfg = chan_cfg.with_max_response_payload_size(77);
        assert_eq!(chan_cfg.request_limit, 123);
        assert_eq!(chan_cfg.max_request_payload_size, 99);
        assert_eq!(chan_cfg.max_response_payload_size, 77);
    }
}
