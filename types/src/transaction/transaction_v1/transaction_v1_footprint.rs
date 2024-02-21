#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{Gas, TransactionV1Header};

use super::transaction_v1_category::TransactionV1Category;

/// Information about how much block limit a [`Transaction V1`] will consume.
#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct TransactionV1Footprint {
    /// The header of the `Transaction`.
    pub header: TransactionV1Header,
    /// The estimated gas consumption of the `Transaction`.
    pub gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Transaction`.
    pub size_estimate: usize,
    /// Transaction category.
    pub category: TransactionV1Category,
}

impl TransactionV1Footprint {
    /// Returns true if transaction has been categorized as install/upgrade.
    pub fn is_install_upgrade(&self) -> bool {
        matches!(self.category, TransactionV1Category::InstallUpgrade)
    }

    /// Returns true if transaction has been categorized as standard.
    pub fn is_standard(&self) -> bool {
        matches!(self.category, TransactionV1Category::Standard)
    }

    /// Returns true if transaction has been categorized as staking.
    pub fn is_staking(&self) -> bool {
        matches!(self.category, TransactionV1Category::Staking)
    }

    /// Returns true if transaction has been categorized as transfer.
    pub fn is_transfer(&self) -> bool {
        matches!(self.category, TransactionV1Category::Transfer)
    }
}
