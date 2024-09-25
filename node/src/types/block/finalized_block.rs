use std::{
    cmp::{Ord, PartialOrd},
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    hash::Hash,
};

#[cfg(test)]
use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::{SecretKey, Transaction};
#[cfg(test)]
use {casper_types::testing::TestRng, rand::Rng};

use casper_types::{
    BlockV2, EraId, PublicKey, RewardedSignatures, Timestamp, TransactionHash, AUCTION_LANE_ID,
    INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
};

use super::BlockPayload;

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub(crate) transactions: BTreeMap<u8, Vec<TransactionHash>>,
    pub(crate) rewarded_signatures: RewardedSignatures,
    pub(crate) timestamp: Timestamp,
    pub(crate) random_bit: bool,
    pub(crate) era_report: Option<InternalEraReport>,
    pub(crate) era_id: EraId,
    pub(crate) height: u64,
    pub(crate) proposer: Box<PublicKey>,
    pub(crate) current_gas_price: u8,
}

/// `EraReport` used only internally. The one in types is a part of `EraEndV1`.
#[derive(
    Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
pub struct InternalEraReport {
    /// The set of equivocators.
    pub equivocators: Vec<PublicKey>,
    /// Validators that haven't produced any unit during the era.
    pub inactive_validators: Vec<PublicKey>,
}

impl FinalizedBlock {
    pub(crate) fn new(
        block_payload: BlockPayload,
        era_report: Option<InternalEraReport>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        proposer: PublicKey,
    ) -> Self {
        let current_gas_price = block_payload.current_gas_price();
        let transactions = block_payload.finalized_payload();

        FinalizedBlock {
            transactions,
            rewarded_signatures: block_payload.rewarded_signatures().clone(),
            timestamp,
            random_bit: block_payload.random_bit(),
            era_report,
            era_id,
            height,
            proposer: Box::new(proposer),
            current_gas_price,
        }
    }

    pub(crate) fn mint(&self) -> Vec<TransactionHash> {
        self.transactions
            .get(&MINT_LANE_ID)
            .map(|transactions| transactions.to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn auction(&self) -> Vec<TransactionHash> {
        self.transactions
            .get(&AUCTION_LANE_ID)
            .map(|transactions| transactions.to_vec())
            .unwrap_or_default()
    }
    pub(crate) fn install_upgrade(&self) -> Vec<TransactionHash> {
        self.transactions
            .get(&INSTALL_UPGRADE_LANE_ID)
            .map(|transactions| transactions.to_vec())
            .unwrap_or_default()
    }

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub(crate) fn all_transactions(&self) -> impl Iterator<Item = &TransactionHash> {
        self.transactions.values().flatten()
    }

    /// Generates a random instance using a `TestRng` and includes specified deploys.
    #[cfg(test)]
    pub(crate) fn random<'a, I: IntoIterator<Item = &'a Transaction>>(
        rng: &mut TestRng,
        txns_iter: I,
    ) -> Self {
        let era = rng.gen_range(0..5);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);

        FinalizedBlock::random_with_specifics(
            rng,
            EraId::from(era),
            height,
            is_switch,
            Timestamp::now(),
            txns_iter,
        )
    }

    /// Generates a random instance using a `TestRng`, but using the specified values.
    /// If `deploy` is `None`, random deploys will be generated, otherwise, the provided `deploy`
    /// will be used.
    #[cfg(test)]
    pub(crate) fn random_with_specifics<'a, I: IntoIterator<Item = &'a Transaction>>(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        is_switch: bool,
        timestamp: Timestamp,
        txns_iter: I,
    ) -> Self {
        let mut transactions = BTreeMap::new();
        let mut standard = vec![];
        for transaction in txns_iter {
            standard.push((transaction.hash(), BTreeSet::new()));
        }
        transactions.insert(3, standard);
        let rewarded_signatures = Default::default();
        let random_bit = rng.gen();
        let block_payload =
            BlockPayload::new(transactions, vec![], rewarded_signatures, random_bit, 1u8);

        let era_report = if is_switch {
            Some(InternalEraReport::random(rng))
        } else {
            None
        };
        let secret_key: SecretKey = SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();
        let public_key = PublicKey::from(&secret_key);

        FinalizedBlock::new(
            block_payload,
            era_report,
            timestamp,
            era_id,
            height,
            public_key,
        )
    }
}

impl From<BlockV2> for FinalizedBlock {
    fn from(block: BlockV2) -> Self {
        FinalizedBlock {
            transactions: block.transactions().clone(),
            timestamp: block.timestamp(),
            random_bit: block.random_bit(),
            era_report: block.era_end().map(|era_end| InternalEraReport {
                equivocators: Vec::from(era_end.equivocators()),
                inactive_validators: Vec::from(era_end.inactive_validators()),
            }),
            era_id: block.era_id(),
            height: block.height(),
            proposer: Box::new(block.proposer().clone()),
            rewarded_signatures: block.rewarded_signatures().clone(),
            current_gas_price: block.header().current_gas_price(),
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block #{} in {}, timestamp {}, {} transfers, {} staking txns, {} \
            install/upgrade txns,",
            self.height,
            self.era_id,
            self.timestamp,
            self.mint().len(),
            self.auction().len(),
            self.install_upgrade().len(),
        )?;
        for (category, transactions) in self.transactions.iter() {
            write!(
                formatter,
                "lane: {} has {} transactions",
                category,
                transactions.len()
            )?;
        }
        if let Some(ref ee) = self.era_report {
            write!(formatter, ", era_end: {:?}", ee)?;
        }
        Ok(())
    }
}

impl InternalEraReport {
    /// Returns a random `InternalEraReport`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let equivocators_count = rng.gen_range(0..5);
        let inactive_count = rng.gen_range(0..5);
        let equivocators = core::iter::repeat_with(|| PublicKey::random(rng))
            .take(equivocators_count)
            .collect();
        let inactive_validators = core::iter::repeat_with(|| PublicKey::random(rng))
            .take(inactive_count)
            .collect();

        InternalEraReport {
            equivocators,
            inactive_validators,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::Deploy;

    #[test]
    fn should_convert_from_proposable_to_finalized_without_dropping_hashes() {
        let mut rng = TestRng::new();

        let large_lane_id = 3;
        let standard = Transaction::Deploy(Deploy::random(&mut rng));
        let hash = standard.hash();
        let transactions = {
            let mut ret = BTreeMap::new();
            ret.insert(large_lane_id, vec![(hash, BTreeSet::new())]);
            ret.insert(MINT_LANE_ID, vec![]);
            ret.insert(INSTALL_UPGRADE_LANE_ID, vec![]);
            ret.insert(AUCTION_LANE_ID, vec![]);
            ret
        };
        let block_payload = BlockPayload::new(transactions, vec![], Default::default(), false, 1u8);

        let fb = FinalizedBlock::new(
            block_payload,
            None,
            Timestamp::now(),
            EraId::random(&mut rng),
            90,
            PublicKey::random(&mut rng),
        );

        let transactions = fb.transactions.get(&large_lane_id).unwrap();
        assert!(!transactions.is_empty())
    }
}
