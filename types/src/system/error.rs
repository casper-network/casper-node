use core::fmt::{self, Display, Formatter};

use crate::system::{auction, handle_payment, mint};

/// An aggregate enum error with variants for each system contract's error.
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub enum Error {
    /// Contains a [`mint::Error`].
    Mint(mint::Error),
    /// Contains a [`handle_payment::Error`].
    HandlePayment(handle_payment::Error),
    /// Contains a [`auction::Error`].
    Auction(auction::Error),
}

impl From<mint::Error> for Error {
    fn from(error: mint::Error) -> Error {
        Error::Mint(error)
    }
}

impl From<handle_payment::Error> for Error {
    fn from(error: handle_payment::Error) -> Error {
        Error::HandlePayment(error)
    }
}

impl From<auction::Error> for Error {
    fn from(error: auction::Error) -> Error {
        Error::Auction(error)
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::Mint(error) => write!(formatter, "Mint error: {}", error),
            Error::HandlePayment(error) => write!(formatter, "HandlePayment error: {}", error),
            Error::Auction(error) => write!(formatter, "Auction error: {}", error),
        }
    }
}
