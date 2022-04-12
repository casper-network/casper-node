use std::collections::HashSet;

use casper_types::{Gas, PublicKey};
use datasize::DataSize;
use num_traits::Zero;
use thiserror::Error;

use crate::{
    components::block_proposer::DeployInfo,
    types::{chainspec::DeployConfig, BlockPayload, DeployHash, DeployWithApprovals, Timestamp},
};

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
    #[error("payment amount could not be converted to gas")]
    InvalidGasAmount,
    #[error("deploy is not valid in this context")]
    InvalidDeploy,
}

/// A block that is still being added to. It keeps track of and enforces block limits.
#[derive(Clone, DataSize, Debug)]
pub struct AppendableBlock {
    deploy_config: DeployConfig,
    deploys: Vec<DeployWithApprovals>,
    transfers: Vec<DeployWithApprovals>,
    deploy_and_transfer_set: HashSet<DeployHash>,
    timestamp: Timestamp,
    #[data_size(skip)]
    total_gas: Gas,
    total_size: usize,
    total_approvals: usize,
}

impl AppendableBlock {
    /// Creates an empty `AppendableBlock`.
    pub(crate) fn new(deploy_config: DeployConfig, timestamp: Timestamp) -> Self {
        AppendableBlock {
            deploy_config,
            deploys: Vec::new(),
            transfers: Vec::new(),
            timestamp,
            deploy_and_transfer_set: HashSet::new(),
            total_gas: Gas::zero(),
            total_size: 0,
            total_approvals: 0,
        }
    }

    /// Returns the total size of all deploys so far.
    pub(crate) fn total_size(&self) -> usize {
        self.total_size
    }

    /// Attempts to add a transfer to the block; returns an error if that would violate a validity
    /// condition.
    ///
    /// This _must_ be called with a transfer - the function cannot check whether the argument is
    /// actually a transfer.
    pub(crate) fn add_transfer(
        &mut self,
        transfer: DeployWithApprovals,
        deploy_info: &DeployInfo,
    ) -> Result<(), AddError> {
        if self
            .deploy_and_transfer_set
            .contains(transfer.deploy_hash())
        {
            return Err(AddError::Duplicate);
        }
        if !deploy_info
            .header
            .is_valid(&self.deploy_config, self.timestamp)
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
        deploy: DeployWithApprovals,
        deploy_info: &DeployInfo,
    ) -> Result<(), AddError> {
        if self.deploy_and_transfer_set.contains(deploy.deploy_hash()) {
            return Err(AddError::Duplicate);
        }
        if !deploy_info
            .header
            .is_valid(&self.deploy_config, self.timestamp)
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
            .checked_add(deploy_info.size)
            .filter(|size| *size <= self.deploy_config.max_block_size as usize)
            .ok_or(AddError::BlockSize)?;
        let payment_amount = deploy_info.payment_amount;
        let gas_price = deploy_info.header.gas_price();
        let gas = Gas::from_motes(payment_amount, gas_price).ok_or(AddError::InvalidGasAmount)?;
        let new_total_gas = self.total_gas.checked_add(gas).ok_or(AddError::GasLimit)?;
        if new_total_gas > Gas::from(self.deploy_config.block_gas_limit) {
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

    /// Returns `true` if the number of transfers is already the maximum allowed count, i.e. no
    /// more transfers can be added to this block.
    fn has_max_transfer_count(&self) -> bool {
        self.transfers.len() == self.deploy_config.block_max_transfer_count as usize
    }

    /// Returns `true` if the number of deploys is already the maximum allowed count, i.e. no more
    /// deploys can be added to this block.
    fn has_max_deploy_count(&self) -> bool {
        self.deploys.len() == self.deploy_config.block_max_deploy_count as usize
    }

    /// Returns `true` if adding the deploy with 'additional_approvals` approvals would exceed the
    /// approval limits.
    /// Note that we also disallow adding deploys with a number of approvals that would make it
    /// impossible to fill the rest of the block with deploys that have one approval each.
    fn would_exceed_approval_limits(&self, additional_approvals: usize) -> bool {
        let remaining_approval_slots =
            self.deploy_config.block_max_approval_count as usize - self.total_approvals;
        let remaining_deploy_slots = self.deploy_config.block_max_transfer_count as usize
            - self.transfers.len()
            + self.deploy_config.block_max_deploy_count as usize
            - self.deploys.len();
        additional_approvals > remaining_approval_slots - remaining_deploy_slots + 1
    }
}
