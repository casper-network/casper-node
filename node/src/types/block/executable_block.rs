use std::{collections::BTreeMap, fmt};

use datasize::DataSize;
use serde::Serialize;

use casper_types::{
    BlockV2, EraId, PublicKey, RewardedSignatures, Timestamp, Transaction, TransactionHash,
    AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID, U512,
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
    pub(crate) current_gas_price: u8,
    /// The transactions for the `FinalizedBlock`.
    pub(crate) transactions: Vec<Transaction>,
    pub(crate) transaction_map: BTreeMap<u8, Vec<TransactionHash>>,
    /// `None` may indicate that the rewards have not been computed yet,
    /// or that the block is not a switch one.
    pub(crate) rewards: Option<BTreeMap<PublicKey, Vec<U512>>>,
    /// `None` may indicate that the next era gas has not been computed yet,
    /// or that the block is not a switch one.
    pub(crate) next_era_gas_price: Option<u8>,
}

impl ExecutableBlock {
    pub(crate) fn mint(&self) -> Vec<TransactionHash> {
        self.transaction_map
            .get(&MINT_LANE_ID)
            .cloned()
            .unwrap_or(vec![])
    }

    pub(crate) fn auction(&self) -> Vec<TransactionHash> {
        self.transaction_map
            .get(&AUCTION_LANE_ID)
            .cloned()
            .unwrap_or(vec![])
    }

    pub(crate) fn install_upgrade(&self) -> Vec<TransactionHash> {
        self.transaction_map
            .get(&INSTALL_UPGRADE_LANE_ID)
            .cloned()
            .unwrap_or(vec![])
    }

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
            transaction_map: finalized_block.transactions,
            rewards: None,
            next_era_gas_price: None,
            current_gas_price: finalized_block.current_gas_price,
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
            transaction_map: block.transactions().clone(),
            rewards: block.era_end().map(|era_end| era_end.rewards().clone()),
            next_era_gas_price: block.era_end().map(|era_end| era_end.next_era_gas_price()),
            current_gas_price: block.header().current_gas_price(),
        }
    }
}

impl fmt::Display for ExecutableBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executable block #{} in {}, timestamp {}, {} transfers, {} staking txns, {} \
            install/upgrade txns",
            self.height,
            self.era_id,
            self.timestamp,
            self.mint().len(),
            self.auction().len(),
            self.install_upgrade().len(),
        )?;
        for (lane, wasm_transaction) in self.transaction_map.iter() {
            if *lane < 3 {
                continue;
            }
            write!(
                formatter,
                ", lane: {} with {} transactions",
                *lane,
                wasm_transaction.len()
            )?;
        }
        if let Some(ref ee) = self.era_report {
            write!(formatter, ", era_end: {:?}", ee)?;
        }
        if let Some(ref next_era_gas_price) = self.next_era_gas_price {
            write!(formatter, ", next_era_gas_price: {}", next_era_gas_price)?;
        }
        Ok(())
    }
}
