use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use num_traits::Zero;
use thiserror::Error;
use tracing::error;

use casper_types::{
    DeployFootprint, Gas, PublicKey, RewardedSignatures, TimeDiff, Timestamp, TransactionConfig,
    TransactionFootprint, TransactionHash, TransactionV1Category, TransactionV1Footprint,
};

use super::{
    transaction::{DeployHashWithApprovals, TransactionV1HashWithApprovals},
    BlockPayload,
};
use crate::types::TransactionHashWithApprovals;

const NO_LEEWAY: TimeDiff = TimeDiff::from_millis(0);

#[derive(Debug, Error)]
pub(crate) enum MismatchType {
    #[error("legacy deploy with V1 footprint")]
    LegacyDeployWithV1Footprint,
    #[error("V1 transaction with legacy footprint")]
    V1TransactionWithLegacyDeployFootprint,
}

#[derive(Debug, Error)]
pub(crate) enum AddError {
    #[error("would exceed maximum transfer count per block")]
    TransferCount,
    #[error("would exceed maximum deploy count per block")]
    DeployCount,
    #[error("would exceed maximum transaction ({0}) count per block")]
    TransactionCount(TransactionV1Category),
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
    #[error("footprint type mismatch: {0}")]
    FootprintTypeMismatch(MismatchType),
}

/// A block that is still being added to. It keeps track of and enforces block limits.
#[derive(Clone, Eq, PartialEq, DataSize, Debug)]
pub(crate) struct AppendableBlock {
    transaction_config: TransactionConfig,
    transactions: Vec<(TransactionHashWithApprovals, TransactionV1Category)>,
    transaction_and_transfer_set: HashSet<TransactionHash>,
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
            //transfers: Vec::new(),
            timestamp,
            transaction_and_transfer_set: HashSet::new(),
            total_gas: Gas::zero(),
            total_size: 0,
            total_approvals: 0,
        }
    }

    /// Attempts to add any kind of deploy (transfer or other kind).
    pub(crate) fn add(
        &mut self,
        transaction_hash_with_approvals: TransactionHashWithApprovals,
        footprint: &TransactionFootprint,
    ) -> Result<(), AddError> {
        match (transaction_hash_with_approvals, footprint) {
            (
                TransactionHashWithApprovals::Deploy {
                    deploy_hash,
                    approvals,
                },
                TransactionFootprint::Deploy(deploy_footprint),
            ) => {
                let dhwa = DeployHashWithApprovals::new(deploy_hash, approvals);
                if footprint.is_transfer() {
                    self.add_transfer(dhwa, deploy_footprint)
                } else {
                    self.add_deploy(dhwa, deploy_footprint)
                }
            }
            (
                TransactionHashWithApprovals::V1(thwa),
                TransactionFootprint::V1(transaction_footprint),
            ) => self.add_transaction_v1(thwa, transaction_footprint),
            (TransactionHashWithApprovals::V1 { .. }, TransactionFootprint::Deploy(_)) => {
                Err(AddError::FootprintTypeMismatch(
                    MismatchType::V1TransactionWithLegacyDeployFootprint,
                ))
            }
            (TransactionHashWithApprovals::Deploy { .. }, TransactionFootprint::V1(_)) => Err(
                AddError::FootprintTypeMismatch(MismatchType::LegacyDeployWithV1Footprint),
            ),
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
        footprint: &DeployFootprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_and_transfer_set
            .contains(&TransactionHash::from(transfer.deploy_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if footprint
            .header
            .is_valid(
                &self.transaction_config,
                NO_LEEWAY,
                self.timestamp,
                transfer.deploy_hash(),
            )
            .is_err()
        {
            return Err(AddError::InvalidDeploy);
        }
        if self.has_max_transfer_count() {
            return Err(AddError::TransferCount);
        }
        if self.would_exceed_approval_limits(transfer.approvals().len()) {
            return Err(AddError::ApprovalCount);
        }
        self.transaction_and_transfer_set
            .insert(TransactionHash::from(transfer.deploy_hash()));
        self.total_approvals += transfer.approvals().len();
        self.transactions.push((
            TransactionHashWithApprovals::from(transfer),
            TransactionV1Category::Transfer,
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
        footprint: &DeployFootprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_and_transfer_set
            .contains(&TransactionHash::from(deploy.deploy_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if footprint
            .header
            .is_valid(
                &self.transaction_config,
                NO_LEEWAY,
                self.timestamp,
                deploy.deploy_hash(),
            )
            .is_err()
        {
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
        self.transaction_and_transfer_set
            .insert(TransactionHash::from(deploy.deploy_hash()));
        self.transactions
            .push((deploy.into(), TransactionV1Category::Standard));
        Ok(())
    }

    /// Attempts to add a transaction V1 to the block; returns an error if that would violate a
    /// validity condition.
    pub(crate) fn add_transaction_v1(
        &mut self,
        transaction: TransactionV1HashWithApprovals,
        footprint: &TransactionV1Footprint,
    ) -> Result<(), AddError> {
        if self
            .transaction_and_transfer_set
            .contains(&TransactionHash::from(transaction.transaction_hash()))
        {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if footprint
            .header
            .is_valid(
                &self.transaction_config,
                NO_LEEWAY,
                self.timestamp,
                transaction.transaction_hash(),
            )
            .is_err()
        {
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
        self.transaction_and_transfer_set
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
            .filter(|(_, c)| c == &TransactionV1Category::Transfer)
            .map(|(t, _)| t.clone())
            .collect();

        let staking = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionV1Category::Staking)
            .map(|(t, _)| t.clone())
            .collect();

        let install_upgrade = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionV1Category::InstallUpgrade)
            .map(|(t, _)| t.clone())
            .collect();

        let standard = transactions
            .iter()
            .filter(|(_, c)| c == &TransactionV1Category::Standard)
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

    fn category_count(&self, category: &TransactionV1Category) -> usize {
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

    fn category_approval_count(&self, category: &TransactionV1Category) -> usize {
        self.transactions
            .iter()
            .filter(|(_t, c)| c == category)
            .map(|(t, _c)| t.approvals().len())
            .sum()
    }

    /// Returns `true` if the number of transfers is already the maximum allowed count, i.e. no
    /// more transfers can be added to this block.
    fn has_max_transfer_count(&self) -> bool {
        self.category_count(&TransactionV1Category::Transfer)
            == self.transaction_config.block_max_transfer_count as usize
    }

    /// Returns `true` if the number of transactions is already the maximum allowed count, i.e. no
    /// more transactions can be added to this block.
    fn has_max_standard_count(&self) -> bool {
        self.transactions.len() == self.transaction_config.block_max_standard_count as usize
    }

    /// Returns `true` if adding the deploy with 'additional_approvals` approvals would exceed the
    /// approval limits.
    /// Note that we also disallow adding deploys with a number of approvals that would make it
    /// impossible to fill the rest of the block with deploys that have one approval each.
    fn would_exceed_approval_limits(&self, additional_approvals: usize) -> bool {
        let remaining_approval_slots =
            self.transaction_config.block_max_approval_count as usize - self.total_approvals;
        let remaining_deploy_slots = self.transaction_config.block_max_transfer_count as usize
            - self.category_count(&TransactionV1Category::Transfer)
            + self.transaction_config.block_max_standard_count as usize
            - self.transactions.len();
        // safe to subtract because the chainspec is validated at load time
        additional_approvals > remaining_approval_slots - remaining_deploy_slots + 1
    }

    fn has_max_category_count(&self, categbory: &TransactionV1Category) -> bool {
        let category_count = self.category_count(categbory);
        match categbory {
            TransactionV1Category::InstallUpgrade => {
                category_count == self.transaction_config.block_max_install_upgrade_count as usize
            }
            TransactionV1Category::Staking => {
                category_count == self.transaction_config.block_max_staking_count as usize
            }
            TransactionV1Category::Standard => {
                self.transactions.len() == self.transaction_config.block_max_standard_count as usize
            }
            // TODO[RC]: Handle these.
            TransactionV1Category::Transfer => todo!(),
            TransactionV1Category::Other => todo!(),
        }
    }
}

impl Display for AppendableBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let standard_count = self.category_count(&TransactionV1Category::Standard);
        let standard_approvals_count =
            self.category_approval_count(&TransactionV1Category::Standard);
        let transfers_count = self.category_count(&TransactionV1Category::Transfer);
        let transfers_approvals_count =
            self.category_approval_count(&TransactionV1Category::Transfer);
        let staking_count = self.category_count(&TransactionV1Category::Staking);
        let staking_approvals_count = self.category_approval_count(&TransactionV1Category::Staking);
        let install_upgrade_count = self.category_count(&TransactionV1Category::InstallUpgrade);
        let install_upgrade_approvals_count =
            self.category_approval_count(&TransactionV1Category::InstallUpgrade);
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
            &self.transaction_and_transfer_set
        }
    }
}
