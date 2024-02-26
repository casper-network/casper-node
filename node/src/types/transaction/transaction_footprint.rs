use casper_types::{Digest, Transaction};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::{
    error::TransactionError, DeployExt, DeployFootprint, TransactionV1Ext, TransactionV1Footprint,
};

#[derive(Clone, Debug, DataSize, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A footprint of a transaction.
pub(crate) enum TransactionFootprint {
    /// The legacy, initial version of the deploy (v1).
    Deploy(DeployFootprint),
    /// The version 2 of the deploy, aka Transaction.
    V1(TransactionV1Footprint),
}

impl From<DeployFootprint> for TransactionFootprint {
    fn from(value: DeployFootprint) -> Self {
        Self::Deploy(value)
    }
}

impl From<TransactionV1Footprint> for TransactionFootprint {
    fn from(value: TransactionV1Footprint) -> Self {
        Self::V1(value)
    }
}

impl TransactionFootprint {
    /// Returns `true` if this transaction is a transfer.
    pub(crate) fn is_transfer(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => deploy_footprint.is_transfer,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_transfer(),
        }
    }

    /// Returns `true` if this transaction is standard.
    pub(crate) fn is_standard(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => !deploy_footprint.is_transfer,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_standard(),
        }
    }

    /// Returns `true` if this transaction is install upgrade.
    pub(crate) fn is_install_upgrade(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(_) => false,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_install_upgrade(),
        }
    }

    /// Returns `true` if this transaction is staking.
    pub(crate) fn is_staking(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(_) => false,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_staking(),
        }
    }

    /// Returns body hash.
    pub(crate) fn body_hash(&self) -> &Digest {
        match self {
            TransactionFootprint::Deploy(deploy) => deploy.header.body_hash(),
            TransactionFootprint::V1(v1) => v1.header.body_hash(),
        }
    }
}

pub(crate) trait TransactionExt {
    fn footprint(&self) -> Result<TransactionFootprint, TransactionError>;
}

impl TransactionExt for Transaction {
    /// Returns the `TransactionFootprint`.
    fn footprint(&self) -> Result<TransactionFootprint, TransactionError> {
        Ok(match self {
            Transaction::Deploy(deploy) => deploy.footprint()?.into(),
            Transaction::V1(v1) => v1.footprint()?.into(),
        })
    }
}
