use core::fmt::{self, Formatter};

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
pub(crate) enum TransactionCategory {
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
    /// Native entity interaction.
    Entity = 6,
}

impl fmt::Display for TransactionCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionCategory::Mint => write!(f, "Mint"),
            TransactionCategory::Auction => write!(f, "Auction"),
            TransactionCategory::InstallUpgrade => write!(f, "InstallUpgrade"),
            TransactionCategory::Large => write!(f, "Large"),
            TransactionCategory::Medium => write!(f, "Medium"),
            TransactionCategory::Small => write!(f, "Small"),
            TransactionCategory::Entity => write!(f, "Entity"),
        }
    }
}
