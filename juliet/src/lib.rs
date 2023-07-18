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
#[derive(Debug)]
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
#[derive(Copy, Clone, Debug)]
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
        Self {
            request_limit: 1,
            max_request_payload_size: 0,
            max_response_payload_size: 0,
        }
    }
}

impl ChannelConfiguration {
    /// Creates a configuration with the given request limit (default is 1).
    pub fn with_request_limit(mut self, request_limit: u16) -> ChannelConfiguration {
        self.request_limit = request_limit;
        self
    }

    /// Creates a configuration with the given maximum size for request payloads (default is 0).
    pub fn with_max_request_payload_size(
        mut self,
        max_request_payload_size: u32,
    ) -> ChannelConfiguration {
        self.max_request_payload_size = max_request_payload_size;
        self
    }

    /// Creates a configuration with the given maximum size for response payloads (default is 0).
    pub fn with_max_response_payload_size(
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

    use crate::{ChannelId, Id};

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
}
