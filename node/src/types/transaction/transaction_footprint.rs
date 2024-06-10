use casper_types::{
    Approval, Chainspec, Digest, Gas, GasLimited, InvalidTransaction, InvalidTransactionV1,
    TimeDiff, Timestamp, Transaction, TransactionHash, AUCTION_LANE_ID, ENTITY_LANE_ID,
    INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
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
    /// Transaction body hash.
    pub(crate) body_hash: Digest,
    /// The estimated gas consumption.
    pub(crate) gas_limit: Gas,
    /// The gas tolerance.
    pub(crate) gas_price_tolerance: u8,
    /// The bytesrepr serialized length.
    pub(crate) size_estimate: usize,
    /// The transaction category.
    pub(crate) category: u8,
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
        let gas_price_tolerance = transaction.gas_price_tolerance()?;
        let gas_limit = transaction.gas_limit(chainspec)?;
        let category = transaction.transaction_kind();
        if !chainspec
            .transaction_config
            .transaction_v1_config
            .is_supported(category)
        {
            return Err(InvalidTransaction::V1(
                InvalidTransactionV1::InvalidTransactionKind(category),
            ));
        }
        let transaction_hash = transaction.hash();
        let body_hash = transaction.body_hash();
        let size_estimate = transaction.size_estimate();
        let timestamp = transaction.timestamp();
        let ttl = transaction.ttl();
        let approvals = transaction.approvals();
        Ok(TransactionFootprint {
            transaction_hash,
            body_hash,
            gas_limit,
            gas_price_tolerance,
            size_estimate,
            category,
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
        self.category == MINT_LANE_ID
    }

    /// Is auction interaction.
    pub(crate) fn is_auction(&self) -> bool {
        self.category == AUCTION_LANE_ID
    }

    pub(crate) fn is_install_upgrade(&self) -> bool {
        self.category == INSTALL_UPGRADE_LANE_ID
    }

    /// Is entity interaction.
    pub(crate) fn is_entity(&self) -> bool {
        self.category == ENTITY_LANE_ID
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
