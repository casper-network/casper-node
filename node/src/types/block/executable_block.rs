use std::{collections::BTreeMap, fmt};

use datasize::DataSize;
use serde::Serialize;

use casper_types::{
    BlockV2, EraId, PublicKey, RewardedSignatures, Timestamp, Transaction, TransactionHash, U512,
};

use super::{FinalizedBlock, InternalEraReport};

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
    /// The transactions for the `FinalizedBlock`.
    pub(crate) transactions: Vec<Transaction>,
    /// The hashes of the transfer transactions within the `FinalizedBlock`.
    pub(crate) mint: Vec<TransactionHash>,
    /// The hashes of the auction, native transactions within the `FinalizedBlock`.
    pub(crate) auction: Vec<TransactionHash>,
    /// The hashes of the entity, native transactions within the `FinalizedBlock`.
    pub(crate) entity: Vec<TransactionHash>,
    /// The hashes of the installer/upgrader transactions within the `FinalizedBlock`.
    pub(crate) install_upgrade: Vec<TransactionHash>,
    /// The hashes of all other transactions within the `FinalizedBlock`.
    pub(crate) standard: Vec<TransactionHash>,
    /// `None` may indicate that the rewards have not been computed yet,
    /// or that the block is not a switch one.
    pub(crate) rewards: Option<BTreeMap<PublicKey, U512>>,
    /// `None` may indicate that the next era gas has not been computed yet,
    /// or that the block is not a switch one.
    pub(crate) next_era_gas_price: Option<u8>,
}

impl ExecutableBlock {
    /// Creates a new `ExecutedBlock` from a `FinalizedBlock` and its transactions.
    pub fn from_finalized_block_and_transactions(
        finalized_block: FinalizedBlock,
        transactions: Vec<Transaction>,
    ) -> Self {
        Self {
            rewarded_signatures: finalized_block.rewarded_signatures,
            timestamp: finalized_block.timestamp,
            random_bit: finalized_block.random_bit,
            era_report: finalized_block.era_report,
            era_id: finalized_block.era_id,
            height: finalized_block.height,
            proposer: finalized_block.proposer,
            transactions,
            mint: finalized_block.mint,
            auction: finalized_block.auction,
            entity: finalized_block.entity,
            install_upgrade: finalized_block.install_upgrade,
            standard: finalized_block.standard,
            rewards: None,
            next_era_gas_price: None,
        }
    }

    /// Creates a new `ExecutedBlock` from a `BlockV2` and its deploys.
    pub fn from_block_and_transactions(block: BlockV2, transactions: Vec<Transaction>) -> Self {
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
            transactions,
            mint: block.mint().copied().collect(),
            auction: block.auction().copied().collect(),
            entity: block.entity().copied().collect(),
            install_upgrade: block.install_upgrade().copied().collect(),
            standard: block.standard().copied().collect(),
            rewards: block.era_end().map(|era_end| era_end.rewards().clone()),
            next_era_gas_price: block.era_end().map(|era_end| era_end.next_era_gas_price()),
        }
    }
}

impl fmt::Display for ExecutableBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executable block #{} in {}, timestamp {}, {} transfers, {} staking txns, {} \
            install/upgrade txns, {} standard txns, {} entity txns",
            self.height,
            self.era_id,
            self.timestamp,
            self.mint.len(),
            self.auction.len(),
            self.install_upgrade.len(),
            self.standard.len(),
            self.entity.len(),
        )?;
        if let Some(ref ee) = self.era_report {
            write!(formatter, ", era_end: {:?}", ee)?;
        }
        if let Some(ref next_era_gas_price) = self.next_era_gas_price {
            write!(formatter, ", next_era_gas_price: {}", next_era_gas_price)?;
        }
        Ok(())
    }
}
