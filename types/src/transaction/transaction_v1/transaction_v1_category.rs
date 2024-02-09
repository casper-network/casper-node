use core::fmt::{self, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The category of a [`Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Session kind of a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
// TODO[RC]: Move to separate file
pub enum TransactionV1Category {
    /// Install or Upgrade.
    InstallUpgrade,
    /// Standard transaction.
    Standard,
    /// Staking-related transaction.
    Staking,
    /// Transfer.
    Transfer,
    // TODO[RC]: There should be no Other, we should be able to categorize all transactions
    /// Unspecified.
    Other,
}

impl fmt::Display for TransactionV1Category {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionV1Category::InstallUpgrade => write!(f, "InstallUpgrade"),
            TransactionV1Category::Standard => write!(f, "Standard"),
            TransactionV1Category::Staking => write!(f, "Staking"),
            TransactionV1Category::Transfer => write!(f, "Transfer"),
            TransactionV1Category::Other => write!(f, "Other"),
        }
    }
}
