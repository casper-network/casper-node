use casper_types::{
    bytesrepr::ToBytes, CategorizationError, Deploy, Digest, Gas, Transaction, TransactionCategory,
    TransactionHeader, TransactionV1, U512,
};
use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("invalid payment")]
    InvalidPayment,
    #[error(transparent)]
    Categorization(#[from] CategorizationError),
}

/// Information about how much block limit a [`Deploy`] will consume.
#[derive(Clone, Debug, DataSize, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Footprint {
    /// The header of the `Transaction`.
    pub(crate) header: TransactionHeader,
    /// The estimated gas consumption of the `Transaction`.
    pub(crate) gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Transaction`.
    pub(crate) size_estimate: usize,
    /// Category of the `Transaction`.
    pub(crate) category: TransactionCategory,
}

impl Footprint {
    /// Returns whether the `Deploy` is a transfer or not.
    pub(crate) fn is_transfer(&self) -> bool {
        matches!(self.category, TransactionCategory::Transfer)
    }

    pub(crate) fn is_staking(&self) -> bool {
        matches!(self.category, TransactionCategory::Staking)
    }

    pub(crate) fn is_install_upgrade(&self) -> bool {
        matches!(self.category, TransactionCategory::InstallUpgrade)
    }

    pub(crate) fn is_standard(&self) -> bool {
        matches!(self.category, TransactionCategory::Standard)
    }

    pub(crate) fn body_hash(&self) -> &Digest {
        self.header.body_hash()
    }
}

pub(crate) trait DeployExt {
    fn footprint(&self) -> Result<Footprint, Error>;
}

impl DeployExt for Deploy {
    /// Returns the `Footprint`.
    fn footprint(&self) -> Result<Footprint, Error> {
        let header = self.header().clone();
        let gas_estimate = match self.payment().payment_amount(header.gas_price()) {
            Some(gas) => gas,
            None => {
                return Err(Error::InvalidPayment);
            }
        };
        let size_estimate = self.serialized_length();
        let is_transfer = self.session().is_transfer();
        Ok(Footprint {
            header: header.into(),
            gas_estimate,
            size_estimate,
            category: if is_transfer {
                TransactionCategory::Transfer
            } else {
                TransactionCategory::Standard
            },
        })
    }
}

pub(crate) trait TransactionV1Ext {
    fn footprint(&self) -> Result<Footprint, Error>;
}

impl TransactionV1Ext for TransactionV1 {
    /// Returns the `Footprint`.
    fn footprint(&self) -> Result<Footprint, Error> {
        let header = self.header().clone();

        let size_estimate = self.serialized_length();
        Ok(Footprint {
            header: header.into(),
            size_estimate,
            category: self.category()?,
            gas_estimate: Gas::new(U512::from(0)), // TODO[RC]: Implement gas estimation.
        })
    }
}

pub(crate) trait TransactionExt {
    fn footprint(&self) -> Result<Footprint, Error>;
}

impl TransactionExt for Transaction {
    /// Returns the `Footprint`.
    fn footprint(&self) -> Result<Footprint, Error> {
        match self {
            Transaction::Deploy(deploy) => deploy.footprint(),
            Transaction::V1(v1) => v1.footprint(),
        }
    }
}
