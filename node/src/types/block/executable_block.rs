use super::{FinalizedBlock, InternalEraReport};
use casper_types::{
    BlockV2, Deploy, DeployHash, EraId, PublicKey, RewardedSignatures, Timestamp, U512,
};
use datasize::DataSize;
use serde::Serialize;
use std::{collections::BTreeMap, fmt};

/// Data necessary for a block to be executed.
#[derive(DataSize, Debug, Clone, PartialEq, Serialize)]
pub struct ExecutableBlock {
    pub(crate) rewarded_signatures: RewardedSignatures,
    pub(crate) timestamp: Timestamp,
    pub(crate) random_bit: bool,
    pub(crate) era_report: Option<InternalEraReport>,
    pub(crate) era_id: EraId,
    pub(crate) height: u64,
    pub(crate) proposer: Box<PublicKey>,
    /// The deploys for that `FinalizedBlock`
    pub(crate) deploys: Vec<Deploy>,
    pub(crate) deploy_hashes: Vec<DeployHash>,
    pub(crate) transfer_hashes: Vec<DeployHash>,
    /// `None` may indicate that the rewards have not been computed yet,
    /// or that the block is not a switch one.
    pub(crate) rewards: Option<BTreeMap<PublicKey, U512>>,
}

impl ExecutableBlock {
    /// Creates a new `ExecutedBlock` from a `FinalizedBlock` and its deploys.
    pub fn from_finalized_block_and_deploys(block: FinalizedBlock, deploys: Vec<Deploy>) -> Self {
        Self {
            rewarded_signatures: block.rewarded_signatures,
            timestamp: block.timestamp,
            random_bit: block.random_bit,
            era_report: block.era_report,
            era_id: block.era_id,
            height: block.height,
            proposer: block.proposer,
            deploys,
            deploy_hashes: block.deploy_hashes,
            transfer_hashes: block.transfer_hashes,
            rewards: None,
        }
    }

    /// Creates a new `ExecutedBlock` from a `BlockV2` and its deploys.
    pub fn from_block_and_deploys(block: BlockV2, deploys: Vec<Deploy>) -> Self {
        let era_report = block.era_end().map(|ee| InternalEraReport {
            equivocators: ee.equivocators().into(),
            inactive_validators: ee.inactive_validators().into(),
        });

        Self {
            rewarded_signatures: block.rewarded_signatures().clone(),
            timestamp: block.timestamp(),
            random_bit: block.random_bit(),
            era_report,
            era_id: block.era_id(),
            height: block.height(),
            proposer: Box::new(block.proposer().clone()),
            deploys,
            deploy_hashes: block.deploy_hashes().into(),
            transfer_hashes: block.transfer_hashes().into(),
            rewards: block.era_end().map(|era_end| era_end.rewards().clone()),
        }
    }
}

impl fmt::Display for ExecutableBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block #{} in {}, timestamp {}, {} deploys, {} transfers",
            self.height,
            self.era_id,
            self.timestamp,
            self.deploy_hashes.len(),
            self.transfer_hashes.len(),
        )?;
        if let Some(ref ee) = self.era_report {
            write!(formatter, ", era_end: {:?}", ee)?;
        }
        Ok(())
    }
}
