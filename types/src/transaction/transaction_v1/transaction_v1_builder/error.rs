use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(doc)]
use super::{TransactionV1, TransactionV1Builder};

/// Errors returned while building a [`TransactionV1`] using a [`TransactionV1Builder`].
#[derive(Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum TransactionV1BuilderError {
    /// Failed to build transaction due to missing initiator_addr.
    ///
    /// Call [`TransactionV1Builder::with_initiator_addr`] or
    /// [`TransactionV1Builder::with_secret_key`] before calling [`TransactionV1Builder::build`].
    MissingInitiatorAddr,
    /// Failed to build transaction due to missing chain name.
    ///
    /// Call [`TransactionV1Builder::with_chain_name`] before calling
    /// [`TransactionV1Builder::build`].
    MissingChainName,
}

impl Display for TransactionV1BuilderError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionV1BuilderError::MissingInitiatorAddr => {
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
        }
    }
}

#[cfg(feature = "std")]
impl StdError for TransactionV1BuilderError {}
