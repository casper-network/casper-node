use std::{
    cmp::{Ord, PartialOrd},
    collections::BTreeSet,
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    Approval, Block, Deploy, DeployHash, EraId, EraReport, PublicKey, SecretKey, Timestamp,
};

use super::BlockPayload;
use crate::{rpcs::docs::DocExample, types::DeployHashWithApprovals};

static FINALIZED_BLOCK: Lazy<FinalizedBlock> = Lazy::new(|| {
    let transfer_hashes = vec![*Deploy::doc_example().hash()];
    let random_bit = true;
    let timestamp = *Timestamp::doc_example();
    let secret_key = SecretKey::example();
    let public_key = PublicKey::from(secret_key);
    let block_payload = BlockPayload::new(
        vec![],
        transfer_hashes
            .into_iter()
            .map(|hash| {
                let approval = Approval::create(&hash, secret_key);
                let mut approvals = BTreeSet::new();
                approvals.insert(approval);
                DeployHashWithApprovals::new(hash, approvals)
            })
            .collect(),
        vec![],
        random_bit,
    );
    let era_report = Some(EraReport::<PublicKey>::doc_example().clone());
    let era_id = EraId::from(1);
    let height = 10;
    FinalizedBlock::new(
        block_payload,
        era_report,
        timestamp,
        era_id,
        height,
        public_key,
    )
});

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub(crate) deploy_hashes: Vec<DeployHash>,
    pub(crate) transfer_hashes: Vec<DeployHash>,
    pub(crate) timestamp: Timestamp,
    pub(crate) random_bit: bool,
    pub(crate) era_report: Option<Box<EraReport<PublicKey>>>,
    pub(crate) era_id: EraId,
    pub(crate) height: u64,
    pub(crate) proposer: Box<PublicKey>,
}

impl FinalizedBlock {
    pub(crate) fn new(
        block_payload: BlockPayload,
        era_report: Option<EraReport<PublicKey>>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        proposer: PublicKey,
    ) -> Self {
        FinalizedBlock {
            deploy_hashes: block_payload.deploy_hashes().cloned().collect(),
            transfer_hashes: block_payload.transfer_hashes().cloned().collect(),
            timestamp,
            random_bit: block_payload.random_bit(),
            era_report: era_report.map(Box::new),
            era_id,
            height,
            proposer: Box::new(proposer),
        }
    }

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub(crate) fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes.iter().chain(&self.transfer_hashes)
    }

    /// Generates a random instance using a `TestRng` and includes specified deploys.
    #[cfg(test)]
    pub(crate) fn random<'a, I: IntoIterator<Item = &'a Deploy>>(
        rng: &mut TestRng,
        deploys_iter: I,
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
            deploys_iter,
        )
    }

    /// Generates a random instance using a `TestRng`, but using the specified values.
    /// If `deploy` is `None`, random deploys will be generated, otherwise, the provided `deploy`
    /// will be used.
    #[cfg(test)]
    pub(crate) fn random_with_specifics<'a, I: IntoIterator<Item = &'a Deploy>>(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        is_switch: bool,
        timestamp: Timestamp,
        deploys_iter: I,
    ) -> Self {
        use std::iter;

        let mut deploys = deploys_iter
            .into_iter()
            .map(DeployHashWithApprovals::from)
            .collect::<Vec<_>>();
        if deploys.is_empty() {
            let count = rng.gen_range(0..11);
            deploys.extend(
                iter::repeat_with(|| DeployHashWithApprovals::from(&Deploy::random(rng)))
                    .take(count),
            );
        }
        let random_bit = rng.gen();
        let block_payload = BlockPayload::new(deploys, vec![], vec![], random_bit);

        let era_report = if is_switch {
            Some(EraReport::<PublicKey>::random(rng))
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

impl DocExample for FinalizedBlock {
    fn doc_example() -> &'static Self {
        &FINALIZED_BLOCK
    }
}

impl From<Block> for FinalizedBlock {
    fn from(block: Block) -> Self {
        FinalizedBlock {
            deploy_hashes: block.deploy_hashes().to_vec(),
            transfer_hashes: block.transfer_hashes().to_vec(),
            timestamp: block.timestamp(),
            random_bit: block.random_bit(),
            era_report: block
                .era_end()
                .map(|era_end| Box::new(era_end.era_report().clone())),
            era_id: block.era_id(),
            height: block.height(),
            proposer: Box::new(block.proposer().clone()),
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
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
            write!(formatter, ", era_end: {}", ee)?;
        }
        Ok(())
    }
}
