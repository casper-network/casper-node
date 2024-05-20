use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use itertools::Itertools;
use thiserror::Error;
use tracing::error;

use casper_types::{
    Approval, Gas, PublicKey, RewardedSignatures, Timestamp, TransactionCategory,
    TransactionConfig, TransactionHash, U512,
};

use super::{BlockPayload, TransactionFootprint, VariantMismatch};

#[derive(Debug, Error)]
pub(crate) enum AddError {
    #[error("would exceed maximum count for the category per block")]
    Count(TransactionCategory),
    #[error("would exceed maximum approval count per block")]
    ApprovalCount,
    #[error("would exceed maximum gas per block")]
    GasLimit,
    #[error("would exceed maximum block size")]
    BlockSize,
    #[error("duplicate deploy or transaction")]
    Duplicate,
    #[error("deploy or transaction has expired")]
    Expired,
    #[error(transparent)]
    VariantMismatch(#[from] VariantMismatch),
    #[error("transaction has excessive ttl")]
    ExcessiveTtl,
    #[error("transaction is future dated")]
    FutureDatedDeploy,
}

/// A block that is still being added to. It keeps track of and enforces block limits.
#[derive(Clone, Eq, PartialEq, DataSize, Debug)]
pub(crate) struct AppendableBlock {
    transaction_config: TransactionConfig,
    transactions: BTreeMap<TransactionHash, TransactionFootprint>,
    timestamp: Timestamp,
}

impl AppendableBlock {
    /// Creates an empty `AppendableBlock`.
    pub(crate) fn new(transaction_config: TransactionConfig, timestamp: Timestamp) -> Self {
        AppendableBlock {
            transaction_config,
            transactions: BTreeMap::new(),
            timestamp,
        }
    }

    /// Attempt to append transaction to block.
    pub(crate) fn add_transaction(
        &mut self,
        footprint: TransactionFootprint,
    ) -> Result<(), AddError> {
        if self
            .transactions
            .keys()
            .contains(&footprint.transaction_hash)
        {
            return Err(AddError::Duplicate);
        }
        if footprint.ttl > self.transaction_config.max_ttl {
            return Err(AddError::ExcessiveTtl);
        }
        if footprint.timestamp > self.timestamp {
            return Err(AddError::FutureDatedDeploy);
        }
        let expires = footprint.timestamp.saturating_add(footprint.ttl);
        if expires < self.timestamp {
            return Err(AddError::Expired);
        }
        let limit = match footprint.category {
            TransactionCategory::Large
            | TransactionCategory::Medium
            | TransactionCategory::Small => self.transaction_config.block_max_standard_count,
            TransactionCategory::Mint => self.transaction_config.block_max_mint_count,
            TransactionCategory::Auction => self.transaction_config.block_max_auction_count,
            TransactionCategory::InstallUpgrade => {
                self.transaction_config.block_max_install_upgrade_count
            }
        };
        // check total count by category
        let category = footprint.category;
        let count = self
            .transactions
            .iter()
            .filter(|(_, item)| item.category == category)
            .count();
        if count.checked_add(1).ok_or(AddError::Count(category))? > limit as usize {
            return Err(AddError::Count(category));
        }
        // check total gas
        let gas_limit: U512 = self
            .transactions
            .values()
            .map(|item| item.gas_limit.value())
            .sum();
        if gas_limit
            .checked_add(footprint.gas_limit.value())
            .ok_or(AddError::GasLimit)?
            > U512::from(self.transaction_config.block_gas_limit)
        {
            return Err(AddError::GasLimit);
        }
        // check total byte size
        let size: usize = self
            .transactions
            .values()
            .map(|item| item.size_estimate)
            .sum();
        if size
            .checked_add(footprint.size_estimate)
            .ok_or(AddError::BlockSize)?
            > self.transaction_config.max_block_size as usize
        {
            return Err(AddError::BlockSize);
        }
        // check total approvals
        let count: usize = self
            .transactions
            .values()
            .map(|item| item.approvals_count())
            .sum();
        if count
            .checked_add(footprint.approvals_count())
            .ok_or(AddError::ApprovalCount)?
            > self.transaction_config.block_max_approval_count as usize
        {
            return Err(AddError::ApprovalCount);
        }
        self.transactions
            .insert(footprint.transaction_hash, footprint);
        Ok(())
    }

    /// Creates a `BlockPayload` with the `AppendableBlock`s transactions and transfers, and the
    /// given random bit and accusations.
    pub(crate) fn into_block_payload(
        self,
        accusations: Vec<PublicKey>,
        rewarded_signatures: RewardedSignatures,
        random_bit: bool,
    ) -> BlockPayload {
        let AppendableBlock {
            transactions: footprints,
            ..
        } = self;

        fn collate(
            category: TransactionCategory,
            collater: &mut BTreeMap<
                TransactionCategory,
                Vec<(TransactionHash, BTreeSet<Approval>)>,
            >,
            items: &BTreeMap<TransactionHash, TransactionFootprint>,
        ) {
            let mut ret = vec![];
            for (x, y) in items.iter().filter(|(_, y)| y.category == category) {
                ret.push((*x, y.approvals.clone()));
            }
            if !ret.is_empty() {
                collater.insert(category, ret);
            }
        }

        let mut transactions = BTreeMap::new();
        collate(TransactionCategory::Mint, &mut transactions, &footprints);
        collate(TransactionCategory::Auction, &mut transactions, &footprints);
        collate(TransactionCategory::Large, &mut transactions, &footprints);
        collate(
            TransactionCategory::InstallUpgrade,
            &mut transactions,
            &footprints,
        );

        BlockPayload::new(transactions, accusations, rewarded_signatures, random_bit)
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    fn category_count(&self, category: &TransactionCategory) -> usize {
        self.transactions
            .iter()
            .filter(|(_, f)| f.category == *category)
            .count()
    }
}

impl Display for AppendableBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let standard_count = self.category_count(&TransactionCategory::Large);
        let mint_count = self.category_count(&TransactionCategory::Mint);
        let auction_count = self.category_count(&TransactionCategory::Auction);
        let install_upgrade_count = self.category_count(&TransactionCategory::InstallUpgrade);
        let total_count = self.transactions.len();
        let total_gas_limit: Gas = self.transactions.values().map(|f| f.gas_limit).sum();

        let total_approvals_count: usize = self
            .transactions
            .values()
            .map(|f| f.approvals_count())
            .sum();
        let total_size_estimate: usize = self.transactions.values().map(|f| f.size_estimate).sum();

        write!(
            formatter,
            "AppendableBlock(timestamp-{}:
                standard: {standard_count}, \
                mint: {mint_count}, \
                auction: {auction_count}, \
                install_upgrade: {install_upgrade_count}, \
                total count: {total_count}, \
                approvals: {total_approvals_count}, \
                gas: {total_gas_limit}, \
                size: {total_size_estimate})",
            self.timestamp,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    impl AppendableBlock {
        pub(crate) fn transaction_hashes(&self) -> HashSet<TransactionHash> {
            self.transactions.keys().copied().collect()
        }
    }
}
