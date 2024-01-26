use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    DeployFootprint, TimeDiff, Timestamp, TransactionConfig, TransactionConfigFailure,
    TransactionHash,
};

use super::transaction_v1::TransactionV1Footprint;

#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum TransactionFootprint {
    Deploy(DeployFootprint),
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
    pub fn is_transfer(&self) -> bool {
        match self {
            TransactionFootprint::Deploy(deploy_footprint) => deploy_footprint.is_transfer,
            TransactionFootprint::V1(v1_footprint) => v1_footprint.is_transfer,
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

    /// Returns `Ok` if and only if the transaction is valid.
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
            _ => todo!("programmer error, checking deploy with v1 hash or vice versa"),
        }
    }
}
