//! Home of error types returned by system contracts.
use failure::Fail;

pub mod auction;
pub mod mint;
pub mod pos;

/// An aggregate enum error with variants for each system contract's error.
#[derive(Fail, Debug, Copy, Clone)]
pub enum Error {
    /// Contains a [`mint::Error`].
    #[fail(display = "Mint error: {}", _0)]
    Mint(mint::Error),
    /// Contains a [`pos::Error`].
    #[fail(display = "Proof of Stake error: {}", _0)]
    Pos(pos::Error),
    /// Contains a [`auction::Error`].
    #[fail(display = "Auction error: {}", _0)]
    Auction(auction::Error),
}

impl From<mint::Error> for Error {
    fn from(error: mint::Error) -> Error {
        Error::Mint(error)
    }
}

impl From<pos::Error> for Error {
    fn from(error: pos::Error) -> Error {
        Error::Pos(error)
    }
}

impl From<auction::Error> for Error {
    fn from(error: auction::Error) -> Error {
        Error::Auction(error)
    }
}
