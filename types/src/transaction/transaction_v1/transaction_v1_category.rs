use core::fmt::{self, Formatter};

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
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
}

impl TransactionCategory {
    /// Returns a random transaction category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0u32..4) {
            0 => Self::Mint,
            1 => Self::Auction,
            2 => Self::InstallUpgrade,
            3 => Self::Large,
            4 => Self::Medium,
            5 => Self::Small,
            _ => unreachable!(),
        }
    }
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
        }
    }
}
