#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{DeployFootprint, Digest, Gas, Timestamp};
#[cfg(any(feature = "std", test))]
use crate::{TimeDiff, TransactionConfig, TransactionHash};

#[cfg(any(feature = "std", test))]
use crate::TransactionConfigFailure;

use super::transaction_v1::TransactionV1Footprint;

#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
/// A footprint of a transaction.
pub enum TransactionFootprint {
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
    pub fn is_transfer(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => deploy_footprint.is_transfer,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_transfer(),
        }
    }

    /// Returns `true` if this transaction is standard.
    pub fn is_standard(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => !deploy_footprint.is_transfer,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_standard(),
        }
    }

    /// Returns `true` if this transaction is install upgrade.
    pub fn is_install_upgrade(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(_) => false,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_install_upgrade(),
        }
    }

    /// Returns `true` if this transaction is staking.
    pub fn is_staking(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(_) => false,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_staking(),
        }
    }

    /// Returns gas estimate
    pub fn gas_estimate(&self) -> Gas {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => deploy_footprint.gas_estimate,
            TransactionFootprint::V1(transaction_v1_footprint) => {
                transaction_v1_footprint.gas_estimate
            }
        }
    }

    /// Returns size estimate
    pub fn size_estimate(&self) -> usize {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => deploy_footprint.size_estimate,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.size_estimate,
        }
    }

    /// Returns `true` if the `Transaction` has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => {
                deploy_footprint.header.expired(current_instant)
            }
            TransactionFootprint::V1(v1_footprint) => v1_footprint.header.expired(current_instant),
        }
    }

    /// Returns body hash.
    pub fn body_hash(&self) -> &Digest {
        match self {
            TransactionFootprint::Deploy(deploy) => deploy.header.body_hash(),
            TransactionFootprint::V1(v1) => v1.header.body_hash(),
        }
    }

    /// Returns `Ok` if and only if the transaction is valid.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid(
        &self,
        config: &TransactionConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        transaction_hash: &TransactionHash,
    ) -> Result<(), TransactionConfigFailure> {
        match (self, transaction_hash) {
            (
                TransactionFootprint::Deploy(deploy_footprint),
                TransactionHash::Deploy(deploy_hash),
            ) => deploy_footprint
                .header
                .is_valid(config, timestamp_leeway, at, deploy_hash)
                .map_err(Into::into),
            (TransactionFootprint::V1(v1_footprint), TransactionHash::V1(v1_hash)) => v1_footprint
                .header
                .is_valid(config, timestamp_leeway, at, v1_hash)
                .map_err(Into::into),
            _ => {
                panic!("programmer error, checking legacy deploy with v1 hash or vice versa")
            }
        }
    }
}
