use std::collections::HashSet;

use casper_execution_engine::shared::gas::Gas;
use datasize::DataSize;
use num_traits::Zero;
use thiserror::Error;

use crate::{
    components::block_proposer::DeployType,
    types::{chainspec::DeployConfig, DeployHash, ProtoBlock, Timestamp},
};

#[derive(Debug, Error)]
pub(crate) enum AddError {
    #[error("would exceed maximum transfer count per block")]
    TransferCount,
    #[error("would exceed maximum deploy count per block")]
    DeployCount,
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
    deploy_hashes: Vec<DeployHash>,
    transfer_hashes: Vec<DeployHash>,
    deploy_and_transfer_set: HashSet<DeployHash>,
    timestamp: Timestamp,
    #[data_size(skip)]
    total_gas: Gas,
    total_size: usize,
}

impl AppendableBlock {
    /// Creates an empty `AppendableBlock`.
    pub(crate) fn new(deploy_config: DeployConfig, timestamp: Timestamp) -> Self {
        AppendableBlock {
            deploy_config,
            deploy_hashes: Vec::new(),
            transfer_hashes: Vec::new(),
            timestamp,
            deploy_and_transfer_set: HashSet::new(),
            total_gas: Gas::zero(),
            total_size: 0,
        }
    }

    /// Returns the total size of all deploys so far.
    pub(crate) fn total_size(&self) -> usize {
        self.total_size
    }

    /// Attempts to add a deploy to the block; returns an error if that would violate a validity
    /// condition.
    pub(crate) fn add(
        &mut self,
        hash: DeployHash,
        deploy_type: &DeployType,
    ) -> Result<(), AddError> {
        if self.deploy_and_transfer_set.contains(&hash) {
            return Err(AddError::Duplicate);
        }
        if !deploy_type
            .header()
            .is_valid(&self.deploy_config, self.timestamp)
        {
            return Err(AddError::InvalidDeploy);
        }
        if deploy_type.is_transfer() {
            if self.transfer_hashes.len() == self.deploy_config.block_max_transfer_count as usize {
                return Err(AddError::TransferCount);
            }
            self.transfer_hashes.push(hash);
        } else {
            if self.deploy_hashes.len() == self.deploy_config.block_max_deploy_count as usize {
                return Err(AddError::DeployCount);
            }
            // Only deploys count towards the size and gas limits.
            if self.total_size
                > (self.deploy_config.max_block_size as usize).saturating_sub(deploy_type.size())
            {
                return Err(AddError::BlockSize);
            }
            let payment_amount = deploy_type.payment_amount();
            let gas_price = deploy_type.header().gas_price();
            let gas =
                Gas::from_motes(payment_amount, gas_price).ok_or(AddError::InvalidGasAmount)?;
            let new_total_gas = self.total_gas.checked_add(gas).ok_or(AddError::GasLimit)?;
            if new_total_gas > Gas::from(self.deploy_config.block_gas_limit) {
                return Err(AddError::GasLimit);
            }
            self.deploy_hashes.push(hash);
            self.total_gas = new_total_gas;
            self.total_size += deploy_type.size();
        }
        self.deploy_and_transfer_set.insert(hash);
        Ok(())
    }

    /// Creates a `ProtoBlock` with the `AppendableBlock`s deploys and transfers, and the given
    /// random bit.
    pub(crate) fn into_proto_block(self, random_bit: bool) -> ProtoBlock {
        let AppendableBlock {
            deploy_hashes,
            transfer_hashes,
            timestamp,
            ..
        } = self;
        ProtoBlock::new(deploy_hashes, transfer_hashes, timestamp, random_bit)
    }
}
