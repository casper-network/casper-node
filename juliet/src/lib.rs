use std::fmt::{self, Display};

mod header;
mod reader;
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
