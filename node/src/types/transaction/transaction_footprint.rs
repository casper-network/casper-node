use casper_types::{
    bytesrepr::ToBytes, Approval, Chainspec, Digest, Gas, TimeDiff, Timestamp, Transaction,
    TransactionCategory, TransactionHash,
};
use datasize::DataSize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tracing::error;

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
    pub(crate) gas_tolerance: u64,
    /// The bytesrepr serialized length.
    pub(crate) size_estimate: usize,
    /// The transaction category.
    pub(crate) category: TransactionCategory,
    /// Timestamp of the transaction.
    pub(crate) timestamp: Timestamp,
    /// Time to live for the transaction.
    pub(crate) ttl: TimeDiff,
    /// The approvals.
    pub(crate) approvals: BTreeSet<Approval>,
}

impl TransactionFootprint {
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
        matches!(self.category, TransactionCategory::Mint)
    }

    /// Is auction interaction.
    pub(crate) fn is_auction(&self) -> bool {
        matches!(self.category, TransactionCategory::Auction)
    }

    /// Is standard transaction.
    pub(crate) fn is_standard(&self) -> bool {
        matches!(self.category, TransactionCategory::Standard)
    }

    /// Is install or upgrade transaction.
    pub(crate) fn is_install_upgrade(&self) -> bool {
        matches!(self.category, TransactionCategory::InstallUpgrade)
    }

    pub(crate) fn gas_tolerance(&self) -> u64 {
        self.gas_tolerance
    }
}

pub(crate) trait TransactionExt {
    fn footprint(&self, chainspec: &Chainspec) -> Option<TransactionFootprint>;
}

impl TransactionExt for Transaction {
    /// Returns the `TransactionFootprint`, if able.
    fn footprint(&self, chainspec: &Chainspec) -> Option<TransactionFootprint> {
        let cost_table = &chainspec.system_costs_config;

        // IMPORTANT: block inclusion is always calculated based upon gas price multiple = 1
        // Do not confuse actual cost with retail cost.
        let gas_price: Option<u64> = None;
        let (
            transaction_hash,
            body_hash,
            gas_limit,
            gas_tolerance,
            size_estimate,
            category,
            timestamp,
            ttl,
            approvals,
        ) = match self {
            Transaction::Deploy(deploy) => {
                let transaction_hash = TransactionHash::Deploy(*deploy.hash());
                let body_hash = *deploy.header().body_hash();
                let gas_limit = match deploy.gas_limit(cost_table, gas_price) {
                    Ok(amount) => amount,
                    Err(err) => {
                        error!("{:?}", err);
                        return None;
                    }
                };
                let size_estimate = deploy.serialized_length();
                let category = if deploy.is_transfer() {
                    TransactionCategory::Mint
                } else {
                    TransactionCategory::Standard
                };
                let timestamp = deploy.header().timestamp();
                let ttl = deploy.header().ttl();
                let approvals = self.approvals();
                let gas_tolerance = deploy.header().gas_price();
                (
                    transaction_hash,
                    body_hash,
                    gas_limit,
                    gas_tolerance,
                    size_estimate,
                    category,
                    timestamp,
                    ttl,
                    approvals,
                )
            }
            Transaction::V1(transaction) => {
                let transaction_hash = TransactionHash::V1(*transaction.hash());
                let body_hash = *transaction.header().body_hash();
                let gas_tolernace = transaction.header().gas_tolerance();
                let gas_limit = match transaction.gas_limit(cost_table, gas_price) {
                    Some(amount) => amount,
                    None => {
                        error!(
                            "failed to determine gas limit for transaction {:?}",
                            transaction_hash
                        );
                        return None;
                    }
                };
                let size_estimate = transaction.serialized_length();
                let category = if transaction.is_native_mint() {
                    TransactionCategory::Mint
                } else if transaction.is_native_auction() {
                    TransactionCategory::Auction
                } else if transaction.is_install_or_upgrade() {
                    TransactionCategory::InstallUpgrade
                } else {
                    TransactionCategory::Standard
                };
                let timestamp = transaction.header().timestamp();
                let ttl = transaction.header().ttl();
                let approvals = self.approvals();
                (
                    transaction_hash,
                    body_hash,
                    gas_limit,
                    gas_tolernace,
                    size_estimate,
                    category,
                    timestamp,
                    ttl,
                    approvals,
                )
            }
        };
        Some(TransactionFootprint {
            transaction_hash,
            body_hash,
            gas_limit,
            gas_tolerance,
            size_estimate,
            category,
            timestamp,
            ttl,
            approvals,
        })
    }
}
