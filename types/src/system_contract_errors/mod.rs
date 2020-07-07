//! Home of error types returned by system contracts.
use failure::Fail;

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
