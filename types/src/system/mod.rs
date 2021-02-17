//! System modules, formerly known as "system contracts"

pub mod auction;
pub mod mint;
pub mod proof_of_stake;
pub mod standard_payment;

mod error {
    use failure::Fail;

    use crate::system::{auction, mint, proof_of_stake};

    /// An aggregate enum error with variants for each system contract's error.
    #[derive(Fail, Debug, Copy, Clone)]
    pub enum Error {
        /// Contains a [`mint::Error`].
        #[fail(display = "Mint error: {}", _0)]
        Mint(mint::Error),
        /// Contains a [`pos::Error`].
        #[fail(display = "Proof of Stake error: {}", _0)]
        Pos(proof_of_stake::Error),
        /// Contains a [`auction::Error`].
        #[fail(display = "Auction error: {}", _0)]
        Auction(auction::Error),
    }

    impl From<mint::Error> for Error {
        fn from(error: mint::Error) -> Error {
            Error::Mint(error)
        }
    }

    impl From<proof_of_stake::Error> for Error {
        fn from(error: proof_of_stake::Error) -> Error {
            Error::Pos(error)
        }
    }

    impl From<auction::Error> for Error {
        fn from(error: auction::Error) -> Error {
            Error::Auction(error)
        }
    }
}

pub use error::Error;
