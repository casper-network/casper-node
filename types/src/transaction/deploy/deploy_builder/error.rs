use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(doc)]
use super::{Deploy, DeployBuilder};

/// Errors returned while building a [`Deploy`] using a [`DeployBuilder`].
#[derive(Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum DeployBuilderError {
    /// Failed to build `Deploy` due to missing session account.
    ///
    /// Call [`DeployBuilder::with_account`] or [`DeployBuilder::with_secret_key`] before
    /// calling [`DeployBuilder::build`].
    DeployMissingSessionAccount,
    /// Failed to build `Deploy` due to missing payment code.
    ///
    /// Call [`DeployBuilder::with_standard_payment`] or [`DeployBuilder::with_payment`] before
    /// calling [`DeployBuilder::build`].
    DeployMissingPaymentCode,
}

impl Display for DeployBuilderError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DeployBuilderError::DeployMissingSessionAccount => {
                write!(
                    formatter,
                    "deploy requires session account - use `with_account` or `with_secret_key`"
                )
            }
            DeployBuilderError::DeployMissingPaymentCode => {
                write!(
                    formatter,
                    "deploy requires payment code - use `with_payment` or `with_standard_payment`"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for DeployBuilderError {}
