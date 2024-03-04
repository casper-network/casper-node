use core::fmt::{self, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;

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
}

impl TransactionCategory {
    /// Returns a random transaction category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0u32..4) {
            0 => Self::Mint,
            1 => Self::Auction,
            2 => Self::InstallUpgrade,
            3 => Self::Standard,
            _ => unreachable!()
        }
    }

}

impl fmt::Display for TransactionCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionCategory::Standard => write!(f, "Standard"),
            TransactionCategory::Mint => write!(f, "Mint"),
            TransactionCategory::Auction => write!(f, "Auction"),
            TransactionCategory::InstallUpgrade => write!(f, "InstallUpgrade"),
        }
    }
}
