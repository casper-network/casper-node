use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use num_traits::Zero;
use thiserror::Error;
use tracing::error;

use casper_types::{
    Gas, PublicKey, RewardedSignatures, TimeDiff, Timestamp, TransactionCategory,
    TransactionConfig, TransactionHash,
};

use super::{
    transaction::{DeployHashWithApprovals, TransactionV1HashWithApprovals},
    BlockPayload, Footprint, VariantMismatch,
};
use crate::types::TransactionHashWithApprovals;

const NO_LEEWAY: TimeDiff = TimeDiff::from_millis(0);

#[derive(Debug, Error)]
pub(crate) enum AddError {
    #[error("would exceed maximum transfer count per block")]
    TransferCount,
    #[error("would exceed maximum deploy count per block")]
    DeployCount,
    #[error("would exceed maximum transaction ({0}) count per block")]
    TransactionCount(TransactionCategory),
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
    #[error("deploy is not valid in this context")]
    InvalidDeploy,
    #[error("transaction is not valid in this context")]
    InvalidTransaction,
    #[error(transparent)]
    VariantMismatch(#[from] VariantMismatch),
}

/// A block that is still being added to. It keeps track of and enforces block limits.
#[derive(Clone, Eq, PartialEq, DataSize, Debug)]
pub(crate) struct AppendableBlock {
    transaction_config: TransactionConfig,
    transactions: Vec<(TransactionHashWithApprovals, TransactionCategory)>,
    transaction_hashes: HashSet<TransactionHash>,
    timestamp: Timestamp,
    #[data_size(skip)]
    total_gas: Gas,
    total_size: usize,
    total_approvals: usize,
}

impl AppendableBlock {
    /// Creates an empty `AppendableBlock`.
    pub(crate) fn new(transaction_config: TransactionConfig, timestamp: Timestamp) -> Self {
        AppendableBlock {
            transaction_config,
            transactions: Vec::new(),
            timestamp,
            transaction_hashes: HashSet::new(),
            total_gas: Gas::zero(),
            total_size: 0,
            total_approvals: 0,
        }
    }

    /// Attempts to add any kind of transaction (transfer or other kind).
    pub(crate) fn add(
        &mut self,
        transaction_hash_with_approvals: TransactionHashWithApprovals,
        footprint: &Footprint,
    ) -> Result<(), AddError> {
        match transaction_hash_with_approvals {
            TransactionHashWithApprovals::Deploy {
                deploy_hash,
                approvals,
            } => {
                let dhwa = DeployHashWithApprovals::new(deploy_hash, approvals);
                if footprint.is_transfer() {
                    self.add_transfer(dhwa, footprint)
                } else {
                    self.add_deploy(dhwa, footprint)
                }
            }
            TransactionHashWithApprovals::V1(thwa) => self.add_transaction_v1(thwa, footprint),
        }
    }

    /// Attempts to add a transfer to the block; returns an error if that would violate a validity
    /// condition.
    ///
    /// This _must_ be called with a transfer - the function cannot check whether the argument is
    /// actually a transfer.
    pub(crate) fn add_transfer(
        &mut self,
        transfer: DeployHashWithApprovals,
        footprint: &Footprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_hashes
            .contains(&TransactionHash::from(transfer.deploy_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if !footprint.header.is_valid(
            &self.transaction_config,
            NO_LEEWAY,
            self.timestamp,
            &TransactionHash::from(transfer.deploy_hash()),
        ) {
            return Err(AddError::InvalidDeploy);
        }
        if self.has_max_transfer_count() {
            return Err(AddError::TransferCount);
        }
        if self.would_exceed_approval_limits(transfer.approvals().len()) {
            return Err(AddError::ApprovalCount);
        }
        self.transaction_hashes
            .insert(TransactionHash::from(transfer.deploy_hash()));
        self.total_approvals += transfer.approvals().len();
        self.transactions.push((
            TransactionHashWithApprovals::from(transfer),
            TransactionCategory::Transfer,
        ));
        Ok(())
    }

    /// Attempts to add a deploy to the block; returns an error if that would violate a validity
    /// condition.
    ///
    /// This _must not_ be called with a transfer - the function cannot check whether the argument
    /// is actually not a transfer.
    pub(crate) fn add_deploy(
        &mut self,
        deploy: DeployHashWithApprovals,
        footprint: &Footprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_hashes
            .contains(&TransactionHash::from(deploy.deploy_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if !footprint.header.is_valid(
            &self.transaction_config,
            NO_LEEWAY,
            self.timestamp,
            &TransactionHash::from(deploy.deploy_hash()),
        ) {
            return Err(AddError::InvalidDeploy);
        }
        if self.has_max_standard_count() {
            return Err(AddError::DeployCount);
        }
        if self.would_exceed_approval_limits(deploy.approvals().len()) {
            return Err(AddError::ApprovalCount);
        }
        // Only deploys count towards the size and gas limits.
        let new_total_size = self
            .total_size
            .checked_add(footprint.size_estimate)
            .filter(|size| *size <= self.transaction_config.max_block_size as usize)
            .ok_or(AddError::BlockSize)?;
        let gas_estimate = footprint.gas_estimate;
        let new_total_gas = self
            .total_gas
            .checked_add(gas_estimate)
            .ok_or(AddError::GasLimit)?;
        if new_total_gas > Gas::from(self.transaction_config.block_gas_limit) {
            return Err(AddError::GasLimit);
        }
        self.total_gas = new_total_gas;
        self.total_size = new_total_size;
        self.total_approvals += deploy.approvals().len();
        self.transaction_hashes
            .insert(TransactionHash::from(deploy.deploy_hash()));
        self.transactions
            .push((deploy.into(), TransactionCategory::Standard));
        Ok(())
    }

    /// Attempts to add a transaction V1 to the block; returns an error if that would violate a
    /// validity condition.
    pub(crate) fn add_transaction_v1(
        &mut self,
        transaction: TransactionV1HashWithApprovals,
        footprint: &Footprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_hashes
            .contains(&TransactionHash::from(transaction.transaction_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if !footprint.header.is_valid(
            &self.transaction_config,
            NO_LEEWAY,
            self.timestamp,
            &TransactionHash::from(transaction.transaction_hash()),
        ) {
            return Err(AddError::InvalidTransaction);
        }

        if self.has_max_category_count(&footprint.category) {
            return Err(AddError::TransactionCount(footprint.category));
        }

        if self.would_exceed_approval_limits(transaction.approvals().len()) {
            return Err(AddError::ApprovalCount);
        }

        let new_total_size = self
            .total_size
            .checked_add(footprint.size_estimate)
            .filter(|size| *size <= self.transaction_config.max_block_size as usize)
            .ok_or(AddError::BlockSize)?;
        let gas_estimate = footprint.gas_estimate;
        let new_total_gas = self
            .total_gas
            .checked_add(gas_estimate)
            .ok_or(AddError::GasLimit)?;
        if new_total_gas > Gas::from(self.transaction_config.block_gas_limit) {
            return Err(AddError::GasLimit);
        }
        self.total_gas = new_total_gas;
        self.total_size = new_total_size;
        self.total_approvals += transaction.approvals().len();
        self.transaction_hashes
            .insert(TransactionHash::from(transaction.transaction_hash()));
        self.transactions.push((
            TransactionHashWithApprovals::V1(transaction),
            footprint.category,
        ));
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
        let AppendableBlock { transactions, .. } = self;

        let transfers = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionCategory::Transfer)
            .map(|(t, _)| t.clone())
            .collect();

        let staking = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionCategory::Staking)
            .map(|(t, _)| t.clone())
            .collect();

        let install_upgrade = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionCategory::InstallUpgrade)
            .map(|(t, _)| t.clone())
            .collect();

        let standard = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionCategory::Standard)
            .map(|(t, _)| t.clone())
            .collect();

        BlockPayload::new(
            transfers,
            staking,
            install_upgrade,
            standard,
            accusations,
            rewarded_signatures,
            random_bit,
        )
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    fn category_count(&self, category: &TransactionCategory) -> usize {
        self.transactions
            .iter()
            .filter(|(_t, c)| c == category)
            .count()
    }

    fn total_approval_count(&self) -> usize {
        self.transactions
            .iter()
            .map(|(t, _c)| t.approvals().len())
            .sum()
    }

    fn category_approval_count(&self, category: &TransactionCategory) -> usize {
        self.transactions
            .iter()
            .filter(|(_t, c)| c == category)
            .map(|(t, _c)| t.approvals().len())
            .sum()
    }

    /// Returns `true` if the number of transfers is already the maximum allowed count, i.e. no
    /// more transfers can be added to this block.
    fn has_max_transfer_count(&self) -> bool {
        self.category_count(&TransactionCategory::Transfer)
            == self.transaction_config.block_max_transfer_count as usize
    }

    /// Returns `true` if the number of transactions is already the maximum allowed count, i.e. no
    /// more transactions can be added to this block.
    fn has_max_standard_count(&self) -> bool {
        self.category_count(&TransactionCategory::Standard)
            == self.transaction_config.block_max_standard_count as usize
    }

    /// Returns `true` if adding the transaction with `additional_approvals` approvals would exceed
    /// the approval limits.
    /// Note that we also disallow adding transactions with a number of approvals that would make it
    /// impossible to fill the rest of the block with transactions that have one approval each.
    fn would_exceed_approval_limits(&self, additional_approvals: usize) -> bool {
        let remaining_approval_slots =
            self.transaction_config.block_max_approval_count as usize - self.total_approvals;
        let remaining_transaction_slots = self.transaction_config.block_max_transfer_count as usize
            - self.category_count(&TransactionCategory::Transfer)
            + self.transaction_config.block_max_standard_count as usize
            - self.category_count(&TransactionCategory::Standard)
            + self.transaction_config.block_max_staking_count as usize
            - self.category_count(&TransactionCategory::Staking)
            + self.transaction_config.block_max_install_upgrade_count as usize
            - self.category_count(&TransactionCategory::InstallUpgrade);

        // safe to subtract because the chainspec is validated at load time
        additional_approvals > remaining_approval_slots - remaining_transaction_slots + 1
    }

    fn has_max_category_count(&self, category: &TransactionCategory) -> bool {
        let category_count = self.category_count(category);
        match category {
            TransactionCategory::InstallUpgrade => {
                category_count == self.transaction_config.block_max_install_upgrade_count as usize
            }
            TransactionCategory::Staking => {
                category_count == self.transaction_config.block_max_staking_count as usize
            }
            TransactionCategory::Standard => {
                category_count == self.transaction_config.block_max_standard_count as usize
            }
            TransactionCategory::Transfer => {
                category_count == self.transaction_config.block_max_transfer_count as usize
            }
        }
    }
}

impl Display for AppendableBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let standard_count = self.category_count(&TransactionCategory::Standard);
        let standard_approvals_count = self.category_approval_count(&TransactionCategory::Standard);
        let transfers_count = self.category_count(&TransactionCategory::Transfer);
        let transfers_approvals_count =
            self.category_approval_count(&TransactionCategory::Transfer);
        let staking_count = self.category_count(&TransactionCategory::Staking);
        let staking_approvals_count = self.category_approval_count(&TransactionCategory::Staking);
        let install_upgrade_count = self.category_count(&TransactionCategory::InstallUpgrade);
        let install_upgrade_approvals_count =
            self.category_approval_count(&TransactionCategory::InstallUpgrade);
        let total_count = self.transactions.len();
        let total_approvals_count = self.total_approval_count();

        write!(
            formatter,
            "AppendableBlock(timestamp-{}:
                {standard_count} standard with {standard_approvals_count} approvals, \
                {transfers_count} transfers with {transfers_approvals_count} approvals, \
                {staking_count} staking with {staking_approvals_count} approvals, \
                {install_upgrade_count} install/upgrade with {install_upgrade_approvals_count} approvals, \
                total of {total_count} transactions with {total_approvals_count} approvals, \
                total gas {}, total size {})",
            self.timestamp,
            self.total_gas,
            self.total_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl AppendableBlock {
        pub(crate) fn transaction_and_transfer_set(&self) -> &HashSet<TransactionHash> {
            &self.transaction_hashes
        }
    }
}
