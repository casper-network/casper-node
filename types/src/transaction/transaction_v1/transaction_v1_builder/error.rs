use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(doc)]
use super::{TransactionV1, TransactionV1Builder};

/// Errors returned while building a [`TransactionV1`] using a [`TransactionV1Builder`].
#[derive(Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum TransactionV1BuilderError {
    /// Failed to build transaction due to missing account.
    ///
    /// Call [`TransactionV1Builder::with_account`] or [`TransactionV1Builder::with_secret_key`]
    /// before calling [`TransactionV1Builder::build`].
    MissingAccount,
    /// Failed to build transaction due to missing chain name.
    ///
    /// Call [`TransactionV1Builder::with_chain_name`] before calling
    /// [`TransactionV1Builder::build`].
    MissingChainName,
    /// Failed to build transaction due to missing body.
    ///
    /// Call [`TransactionV1Builder::with_body`] before calling [`TransactionV1Builder::build`].
    MissingBody,
}

impl Display for TransactionV1BuilderError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionV1BuilderError::MissingAccount => {
                write!(
                    formatter,
                    "transaction requires account - use `with_account` or `with_secret_key`"
                )
            }
            TransactionV1BuilderError::MissingChainName => {
                write!(
                    formatter,
                    "transaction requires chain name - use `with_chain_name`"
                )
            }
            TransactionV1BuilderError::MissingBody => {
                write!(formatter, "transaction requires body - use `with_body`")
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for TransactionV1BuilderError {}
