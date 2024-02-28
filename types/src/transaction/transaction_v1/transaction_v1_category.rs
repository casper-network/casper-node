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
    schemars(description = "Category of a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum TransactionCategory {
    /// Install or Upgrade.
    InstallUpgrade,
    /// Standard transaction.
    Standard,
    /// Staking-related transaction.
    Staking,
    /// Transfer.
    Transfer,
}

impl fmt::Display for TransactionCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionCategory::InstallUpgrade => write!(f, "InstallUpgrade"),
            TransactionCategory::Standard => write!(f, "Standard"),
            TransactionCategory::Staking => write!(f, "Staking"),
            TransactionCategory::Transfer => write!(f, "Transfer"),
        }
    }
}
