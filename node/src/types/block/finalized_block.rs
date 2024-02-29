use std::{
    cmp::{Ord, PartialOrd},
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{BlockV2, EraId, PublicKey, RewardedSignatures, Timestamp, TransactionHash};
#[cfg(test)]
use casper_types::{SecretKey, Transaction};
#[cfg(any(feature = "testing", test))]
use {casper_types::testing::TestRng, rand::Rng};

use super::BlockPayload;
use crate::types::TransactionHashWithApprovals;

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub(crate) transfer: Vec<TransactionHash>,
    pub(crate) staking: Vec<TransactionHash>,
    pub(crate) install_upgrade: Vec<TransactionHash>,
    pub(crate) standard: Vec<TransactionHash>,
    pub(crate) rewarded_signatures: RewardedSignatures,
    pub(crate) timestamp: Timestamp,
    pub(crate) random_bit: bool,
    pub(crate) era_report: Option<InternalEraReport>,
    pub(crate) era_id: EraId,
    pub(crate) height: u64,
    pub(crate) proposer: Box<PublicKey>,
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
        FinalizedBlock {
            transfer: block_payload
                .transfer()
                .map(TransactionHashWithApprovals::transaction_hash)
                .collect(),
            staking: block_payload
                .staking()
                .map(TransactionHashWithApprovals::transaction_hash)
                .collect(),
            install_upgrade: block_payload
                .install_upgrade()
                .map(TransactionHashWithApprovals::transaction_hash)
                .collect(),
            standard: block_payload
                .standard()
                .map(TransactionHashWithApprovals::transaction_hash)
                .collect(),
            rewarded_signatures: block_payload.rewarded_signatures().clone(),
            timestamp,
            random_bit: block_payload.random_bit(),
            era_report,
            era_id,
            height,
            proposer: Box::new(proposer),
        }
    }

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub(crate) fn all_transactions(&self) -> impl Iterator<Item = &TransactionHash> {
        self.transfer
            .iter()
            .chain(&self.staking)
            .chain(&self.install_upgrade)
            .chain(&self.standard)
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
        use std::iter;

        let mut txns = txns_iter
            .into_iter()
            .map(TransactionHashWithApprovals::from)
            .collect::<Vec<_>>();
        if txns.is_empty() {
            let count = rng.gen_range(0..11);
            txns.extend(
                iter::repeat_with(|| TransactionHashWithApprovals::from(&Transaction::random(rng)))
                    .take(count),
            );
        }
        let rewarded_signatures = Default::default();
        let random_bit = rng.gen();
        let block_payload = BlockPayload::new(
            txns,
            vec![],
            vec![],
            vec![],
            vec![],
            rewarded_signatures,
            random_bit,
        );

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
            transfer: block.transfer().copied().collect(),
            staking: block.staking().copied().collect(),
            install_upgrade: block.install_upgrade().copied().collect(),
            standard: block.standard().copied().collect(),
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
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block #{} in {}, timestamp {}, {} transfers, {} staking txns, {} \
            install/upgrade txns, {} standard txns",
            self.height,
            self.era_id,
            self.timestamp,
            self.transfer.len(),
            self.staking.len(),
            self.install_upgrade.len(),
            self.standard.len(),
        )?;
        if let Some(ref ee) = self.era_report {
            write!(formatter, ", era_end: {:?}", ee)?;
        }
        Ok(())
    }
}

impl InternalEraReport {
    /// Returns a random `EraReport`.
    #[cfg(any(feature = "testing", test))]
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
