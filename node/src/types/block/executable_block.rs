use super::{FinalizedBlock, InternalEraReport};
use casper_types::{
    BlockV2, Chainspec, EraId, PublicKey, RewardedSignatures, Timestamp, Transaction,
    TransactionHash, AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID, U512,
};
use datasize::DataSize;
use num_rational::Ratio;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
};
use tracing::warn;

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

    pub(crate) fn calc_utilization_score(&self, chainspec: &Chainspec) -> Option<u64> {
        let cfg = &chainspec.transaction_config.transaction_v1_config;
        let per_block_capacity = cfg.get_max_block_count();
        let max_block_size = chainspec.transaction_config.max_block_size as u64;
        let block_gas_limit = chainspec.transaction_config.block_gas_limit;

        let mut has_hit_slot_limit = false;
        let mut transaction_hash_to_lane_id = HashMap::new();

        for (lane_id, transactions) in self.transaction_map.iter() {
            transaction_hash_to_lane_id.extend(
                transactions
                    .iter()
                    .map(|transaction| (transaction, *lane_id)),
            );
            let max_count = cfg.get_max_transaction_count(*lane_id);
            if max_count == transactions.len() as u64 {
                has_hit_slot_limit = true;
            }
        }

        if has_hit_slot_limit {
            Some(100u64)
        } else if self.transactions.is_empty() {
            Some(0u64)
        } else {
            let size_utilization: u64 = {
                let total_size_of_transactions: u64 = self
                    .transactions
                    .iter()
                    .map(|transaction| transaction.size_estimate() as u64)
                    .sum();

                Ratio::new(total_size_of_transactions * 100, max_block_size).to_integer()
            };
            let gas_utilization: u64 = {
                let total_gas_limit: u64 = self
                    .transactions
                    .iter()
                    .map(
                        |transaction| match transaction_hash_to_lane_id.get(&transaction.hash()) {
                            Some(lane_id) => match &transaction.gas_limit(chainspec, *lane_id) {
                                Ok(gas_limit) => gas_limit.value().as_u64(),
                                Err(_) => {
                                    warn!("Unable to determine gas limit");
                                    0u64
                                }
                            },
                            None => {
                                warn!("Unable to determine gas limit");
                                0u64
                            }
                        },
                    )
                    .sum();

                Ratio::new(total_gas_limit * 100, block_gas_limit).to_integer()
            };

            let slot_utilization =
                Ratio::new(self.transactions.len() as u64 * 100, per_block_capacity).to_integer();

            let utilization_scores = [slot_utilization, gas_utilization, size_utilization];

            utilization_scores.iter().max().copied()
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
