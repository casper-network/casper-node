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
    /// Standard transaction (the default).
    #[default]
    Standard = 0,
    /// Native mint interaction.
    Mint = 1,
    /// Native auction interaction.
    Auction = 2,
    /// Install or Upgrade.
    InstallUpgrade = 3,
    /// Entity
    Entity = 4,
}

impl TransactionCategory {
    /// Returns a random transaction category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..5) {
            0 => Self::Mint,
            1 => Self::Auction,
            2 => Self::InstallUpgrade,
            3 => Self::Standard,
            4 => Self::Entity,
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for TransactionCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => write!(f, "Standard"),
            Self::Mint => write!(f, "Mint"),
            Self::Auction => write!(f, "Auction"),
            Self::InstallUpgrade => write!(f, "InstallUpgrade"),
            Self::Entity => write!(f, "Entity"),
        }
    }
}
