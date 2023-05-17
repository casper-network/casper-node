use std::{
    fmt::{self, Display},
    num::NonZeroU32,
};

mod header;
pub(crate) mod multiframe;
// mod reader;
pub mod varint;

/// A channel identifier.
///
/// Newtype wrapper to prevent accidental mixups between regular [`u8`]s and those used as channel
/// IDs. Does not indicate whether or not a channel ID is actually valid, i.e. a channel that
/// exists.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
#[repr(transparent)]
struct ChannelId(u8);

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
struct Id(u16);

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

/// The outcome from a parsing operation over a potentially incomplete buffer.
#[derive(Debug)]
#[must_use]
pub enum Outcome<T, E> {
    /// The given data was incomplete, at least the given amount of additional bytes is needed.
    Incomplete(NonZeroU32),
    /// An fatal error was found in the given input.
    Err(E),
    /// The parse was successful and the underlying buffer has been modified to extract `T`.
    Success(T),
}

impl<T, E> Outcome<T, E> {
    /// Expects the outcome, similar to [`std::result::Result::unwrap`].
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
            Outcome::Incomplete(_) | Outcome::Err(_) => panic!("{}", msg),
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
            Outcome::Err(err) => Outcome::Err(f(err)),
            Outcome::Success(value) => Outcome::Success(value),
        }
    }

    /// Unwraps the outcome, similar to [`std::result::Result::unwrap`].
    ///
    /// Returns the value of [`Outcome::Success`].
    ///
    /// # Panics
    ///
    /// Will panic if the [`Outcome`] is not [`Outcome::Success`].
    #[inline]
    #[track_caller]
    pub fn unwrap(self) -> T {
        match self {
            Outcome::Incomplete(n) => panic!("called unwrap on incomplete({}) outcome", n),
            Outcome::Err(_err) => panic!("called unwrap on error outcome"),
            Outcome::Success(value) => value,
        }
    }
}

/// `try!` for [`Outcome`].
///
/// Will return [`Outcome::Incomplete`] and [`Outcome::Err`] upwards, or unwrap the value found in
/// [`Outcome::Success`].
#[macro_export]
macro_rules! try_outcome {
    ($src:expr) => {
        match $src {
            Outcome::Incomplete(n) => return Outcome::Incomplete(n),
            Outcome::Err(err) => return Outcome::Err(err.into()),
            Outcome::Success(value) => value,
        }
    };
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
