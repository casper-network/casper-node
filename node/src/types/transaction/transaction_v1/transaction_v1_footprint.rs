use casper_types::{
    bytesrepr::ToBytes, Gas, TransactionV1, TransactionV1Category, TransactionV1Error,
    TransactionV1Header, U512,
};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

// TODO[RC]: Footprints and Ext traits to be made pub(crate)?
/// Information about how much block limit a [`Transaction V1`] will consume.
#[derive(Clone, Debug, DataSize, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransactionV1Footprint {
    /// The header of the `Transaction`.
    pub(crate) header: TransactionV1Header,
    /// The estimated gas consumption of the `Deploy`.
    pub(crate) gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Transaction`.
    pub(crate) size_estimate: usize,
    /// Transaction category.
    pub(crate) category: TransactionV1Category,
}

impl TransactionV1Footprint {
    /// Returns true if transaction has been categorized as install/upgrade.
    pub(crate) fn is_install_upgrade(&self) -> bool {
        matches!(self.category, TransactionV1Category::InstallUpgrade)
    }

    /// Returns true if transaction has been categorized as standard.
    pub(crate) fn is_standard(&self) -> bool {
        matches!(self.category, TransactionV1Category::Standard)
    }

    /// Returns true if transaction has been categorized as staking.
    pub(crate) fn is_staking(&self) -> bool {
        matches!(self.category, TransactionV1Category::Staking)
    }

    /// Returns true if transaction has been categorized as transfer.
    pub(crate) fn is_transfer(&self) -> bool {
        matches!(self.category, TransactionV1Category::Transfer)
    }
}

pub(crate) trait TransactionV1Ext {
    fn footprint(&self) -> Result<TransactionV1Footprint, TransactionV1Error>;
}

impl TransactionV1Ext for TransactionV1 {
    /// Returns the `TransactionV1Footprint`.
    fn footprint(&self) -> Result<TransactionV1Footprint, TransactionV1Error> {
        let header = self.header().clone();

        let size_estimate = self.serialized_length();
        Ok(TransactionV1Footprint {
            header,
            size_estimate,
            category: self.category()?,
            gas_estimate: Gas::new(U512::from(0)), // TODO[RC]: Implement gas estimation.
        })
    }
}
