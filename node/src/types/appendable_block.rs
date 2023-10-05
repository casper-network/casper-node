use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use num_traits::Zero;
use thiserror::Error;

use casper_types::{DeployFootprint, DeployHash, Gas, PublicKey, Timestamp, TransactionConfig};

use crate::types::{BlockPayload, DeployHashWithApprovals};

#[derive(Debug, Error)]
pub(crate) enum AddError {
    #[error("would exceed maximum transfer count per block")]
    TransferCount,
    #[error("would exceed maximum deploy count per block")]
    DeployCount,
    #[error("would exceed maximum approval count per block")]
    ApprovalCount,
    #[error("would exceed maximum gas per block")]
    GasLimit,
    #[error("would exceed maximum block size")]
    BlockSize,
    #[error("duplicate deploy")]
    Duplicate,
    #[error("deploy has expired")]
    Expired,
    #[error("deploy is not valid in this context")]
    InvalidDeploy,
}

/// A block that is still being added to. It keeps track of and enforces block limits.
#[derive(Clone, DataSize, Debug)]
pub(crate) struct AppendableBlock {
    transaction_config: TransactionConfig,
    deploys: Vec<DeployHashWithApprovals>,
    transfers: Vec<DeployHashWithApprovals>,
    deploy_and_transfer_set: HashSet<DeployHash>,
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
            deploys: Vec::new(),
            transfers: Vec::new(),
            timestamp,
            deploy_and_transfer_set: HashSet::new(),
            total_gas: Gas::zero(),
            total_size: 0,
            total_approvals: 0,
        }
    }

    /// Attempts to add any kind of deploy (transfer or other kind).
    pub(crate) fn add(
        &mut self,
        deploy_hash_with_approvals: DeployHashWithApprovals,
        footprint: &DeployFootprint,
    ) -> Result<(), AddError> {
        if footprint.is_transfer {
            self.add_transfer(deploy_hash_with_approvals, footprint)
        } else {
            self.add_deploy(deploy_hash_with_approvals, footprint)
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
            .deploy_and_transfer_set
            .contains(transfer.deploy_hash())
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
        self.deploy_and_transfer_set.insert(*transfer.deploy_hash());
        self.total_approvals += transfer.approvals().len();
        self.transfers.push(transfer);
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
        if self.deploy_and_transfer_set.contains(deploy.deploy_hash()) {
            return Err(AddError::Duplicate);
        }
        if footprint.header.expired(self.timestamp) {
            return Err(AddError::Expired);
        }
        if footprint
            .header
            .is_valid(
                &self.transaction_config,
                self.timestamp,
                deploy.deploy_hash(),
            )
            .is_err()
        {
            return Err(AddError::InvalidDeploy);
        }
        if self.has_max_deploy_count() {
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
        self.deploy_and_transfer_set.insert(*deploy.deploy_hash());
        self.deploys.push(deploy);
        Ok(())
    }

    /// Creates a `BlockPayload` with the `AppendableBlock`s deploys and transfers, and the given
    /// random bit and accusations.
    pub(crate) fn into_block_payload(
        self,
        accusations: Vec<PublicKey>,
        random_bit: bool,
    ) -> BlockPayload {
        let AppendableBlock {
            deploys, transfers, ..
        } = self;
        BlockPayload::new(deploys, transfers, accusations, random_bit)
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns `true` if the number of transfers is already the maximum allowed count, i.e. no
    /// more transfers can be added to this block.
    fn has_max_transfer_count(&self) -> bool {
        self.transfers.len() == self.transaction_config.block_max_native_count as usize
    }

    /// Returns `true` if the number of deploys is already the maximum allowed count, i.e. no more
    /// deploys can be added to this block.
    fn has_max_deploy_count(&self) -> bool {
        self.deploys.len() == self.transaction_config.block_max_deploy_count as usize
    }

    /// Returns `true` if adding the deploy with 'additional_approvals` approvals would exceed the
    /// approval limits.
    /// Note that we also disallow adding deploys with a number of approvals that would make it
    /// impossible to fill the rest of the block with deploys that have one approval each.
    fn would_exceed_approval_limits(&self, additional_approvals: usize) -> bool {
        let remaining_approval_slots =
            self.transaction_config.block_max_approval_count as usize - self.total_approvals;
        let remaining_deploy_slots = self.transaction_config.block_max_native_count as usize
            - self.transfers.len()
            + self.transaction_config.block_max_deploy_count as usize
            - self.deploys.len();
        // safe to subtract because the chainspec is validated at load time
        additional_approvals > remaining_approval_slots - remaining_deploy_slots + 1
    }
}

impl Display for AppendableBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let deploy_approvals_count = self
            .deploys
            .iter()
            .map(|deploy_hash_with_approvals| deploy_hash_with_approvals.approvals().len())
            .sum::<usize>();
        let transfer_approvals_count = self
            .transfers
            .iter()
            .map(|deploy_hash_with_approvals| deploy_hash_with_approvals.approvals().len())
            .sum::<usize>();
        write!(
            formatter,
            "AppendableBlock(timestamp-{}: {} non-transfers with {} approvals, {} transfers with {} approvals, \
            total of {} deploys with {} approvals, total gas {}, total size {})",
            self.timestamp,
            self.deploys.len(),
            deploy_approvals_count,
            self.transfers.len(),
            transfer_approvals_count,
            self.deploy_and_transfer_set.len(),
            self.total_approvals,
            self.total_gas,
            self.total_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl AppendableBlock {
        pub(crate) fn deploy_and_transfer_set(&self) -> &HashSet<DeployHash> {
            &self.deploy_and_transfer_set
        }
    }
}
