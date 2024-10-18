use crate::types::MetaTransaction;
use casper_types::{
    Approval, Chainspec, Digest, Gas, InvalidTransaction, InvalidTransactionV1, TimeDiff,
    Timestamp, Transaction, TransactionHash, AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID,
    MINT_LANE_ID,
};
use datasize::DataSize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, DataSize, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// The block footprint of a transaction.
pub(crate) struct TransactionFootprint {
    /// The identifying hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Transaction payload hash.
    pub(crate) payload_hash: Digest,
    /// The estimated gas consumption.
    pub(crate) gas_limit: Gas,
    /// The gas tolerance.
    pub(crate) gas_price_tolerance: u8,
    /// The bytesrepr serialized length.
    pub(crate) size_estimate: usize,
    /// The transaction lane_id.
    pub(crate) lane_id: u8,
    /// Timestamp of the transaction.
    pub(crate) timestamp: Timestamp,
    /// Time to live for the transaction.
    pub(crate) ttl: TimeDiff,
    /// The approvals.
    pub(crate) approvals: BTreeSet<Approval>,
}

impl TransactionFootprint {
    pub(crate) fn new(
        chainspec: &Chainspec,
        transaction: &Transaction,
    ) -> Result<Self, InvalidTransaction> {
        let transaction = MetaTransaction::from(transaction, &chainspec.transaction_config)?;
        Self::new_from_meta_transaction(chainspec, &transaction)
    }

    fn new_from_meta_transaction(
        chainspec: &Chainspec,
        transaction: &MetaTransaction,
    ) -> Result<Self, InvalidTransaction> {
        let gas_price_tolerance = transaction.gas_price_tolerance()?;
        let gas_limit = transaction.gas_limit(chainspec)?;
        let lane_id = transaction.transaction_lane();
        if !chainspec
            .transaction_config
            .transaction_v1_config
            .is_supported(lane_id)
        {
            return Err(InvalidTransaction::V1(
                InvalidTransactionV1::InvalidTransactionLane(lane_id),
            ));
        }
        let transaction_hash = transaction.hash();
        let size_estimate = transaction.size_estimate();
        let payload_hash = transaction.payload_hash();
        let timestamp = transaction.timestamp();
        let ttl = transaction.ttl();
        let approvals = transaction.approvals();
        Ok(TransactionFootprint {
            transaction_hash,
            payload_hash,
            gas_limit,
            gas_price_tolerance,
            size_estimate,
            lane_id,
            timestamp,
            ttl,
            approvals,
        })
    }

    /// Sets approvals.
    pub(crate) fn with_approvals(mut self, approvals: BTreeSet<Approval>) -> Self {
        self.approvals = approvals;
        self
    }

    /// The approval count, if known.
    pub(crate) fn approvals_count(&self) -> usize {
        self.approvals.len()
    }

    /// Is mint interaction.
    pub(crate) fn is_mint(&self) -> bool {
        if self.lane_id == MINT_LANE_ID {
            return true;
        }

        false
    }

    /// Is auction interaction.
    pub(crate) fn is_auction(&self) -> bool {
        if self.lane_id == AUCTION_LANE_ID {
            return true;
        }

        false
    }

    pub(crate) fn is_install_upgrade(&self) -> bool {
        if self.lane_id == INSTALL_UPGRADE_LANE_ID {
            return true;
        }

        false
    }

    pub(crate) fn is_wasm_based(&self) -> bool {
        if !self.is_mint() && !self.is_auction() && !self.is_install_upgrade() {
            return true;
        }

        false
    }

    pub(crate) fn gas_price_tolerance(&self) -> u8 {
        self.gas_price_tolerance
    }
}
