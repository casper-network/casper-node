use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(doc)]
use super::{Transaction, TransactionBuilder};

/// Errors returned while building a [`Transaction`] using a [`TransactionBuilder`].
#[derive(Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum TransactionBuilderError {
    /// Failed to build `Transaction` due to missing account.
    ///
    /// Call [`TransactionBuilder::with_account`] or [`TransactionBuilder::with_secret_key`] before
    /// calling [`TransactionBuilder::build`].
    MissingAccount,
    /// Failed to build `Transaction` due to missing chain name.
    ///
    /// Call [`TransactionBuilder::with_chain_name`] before calling [`TransactionBuilder::build`].
    MissingChainName,
    /// Failed to build `Transaction` due to missing body.
    ///
    /// Call [`TransactionBuilder::with_body`] before calling [`TransactionBuilder::build`].
    MissingBody,
}

impl Display for TransactionBuilderError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionBuilderError::MissingAccount => {
                write!(
                    formatter,
                    "transaction requires account - use `with_account` or `with_secret_key`"
                )
            }
            TransactionBuilderError::MissingChainName => {
                write!(
                    formatter,
                    "transaction requires chain name - use `with_chain_name`"
                )
            }
            TransactionBuilderError::MissingBody => {
                write!(formatter, "transaction requires body - use `with_body`")
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for TransactionBuilderError {}
