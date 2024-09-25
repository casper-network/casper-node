use core::{
    convert::TryFrom,
    fmt::{self, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The category of a Transaction.
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Session kind of a V1 Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum TransactionLane {
    /// Native mint interaction (the default).
    #[default]
    Mint = 0,
    /// Native auction interaction.
    Auction = 1,
    /// Install or Upgrade.
    InstallUpgrade = 2,
    /// A large Wasm based transaction.
    Large = 3,
    /// A medium Wasm based transaction.
    Medium = 4,
    /// A small Wasm based transaction.
    Small = 5,
}

impl fmt::Display for TransactionLane {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionLane::Mint => write!(f, "Mint"),
            TransactionLane::Auction => write!(f, "Auction"),
            TransactionLane::InstallUpgrade => write!(f, "InstallUpgrade"),
            TransactionLane::Large => write!(f, "Large"),
            TransactionLane::Medium => write!(f, "Medium"),
            TransactionLane::Small => write!(f, "Small"),
        }
    }
}

#[derive(Debug)]
pub struct TransactionCategoryConversionError(u8);

impl fmt::Display for TransactionCategoryConversionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Failed to convert {} into a TransactionCategory", self.0)
    }
}

impl TryFrom<u8> for TransactionLane {
    type Error = TransactionCategoryConversionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Mint),
            1 => Ok(Self::Auction),
            2 => Ok(Self::InstallUpgrade),
            3 => Ok(Self::Large),
            4 => Ok(Self::Medium),
            5 => Ok(Self::Small),
            _ => Err(TransactionCategoryConversionError(value)),
        }
    }
}
