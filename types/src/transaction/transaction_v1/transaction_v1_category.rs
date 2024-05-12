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
pub enum TransactionCategory {
    /// Large wasm sized transaction interactio.
    #[default]
    Large = 0,
    /// Small wasm sized transaction interaction.
    Small = 1,
    /// .
    Medium = 2,
    /// Install or Upgrade.
    InstallUpgrade = 3,
    /// Native mint interaction.
    Mint = 4,
    /// Native auction interaction.
    Auction = 5,
}

impl TransactionCategory {
    /// Returns a random transaction category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0u32..4) {
            0 => Self::Large,
            1 => Self::Small,
            2 => Self::Medium,
            3 => Self::InstallUpgrade,
            4 => Self::Mint,
            5 => Self::Auction,
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for TransactionCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionCategory::Large => write!(f, "Large"),
            TransactionCategory::Mint => write!(f, "Mint"),
            TransactionCategory::Auction => write!(f, "Auction"),
            TransactionCategory::InstallUpgrade => write!(f, "InstallUpgrade"),
            TransactionCategory::Small => {
                write!(f, "Small")
            }
            TransactionCategory::Medium => {
                write!(f, "Medium")
            }
        }
    }
}
