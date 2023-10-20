use std::{
    cmp::{Ord, PartialOrd},
    collections::BTreeSet,
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use casper_types::{
    BlockV2, Deploy, DeployApproval, DeployHash, EraId, PublicKey, RewardedSignatures, SecretKey,
    SingleBlockRewardedSignatures, Timestamp,
};
#[cfg(any(feature = "testing", test))]
use {casper_types::testing::TestRng, rand::Rng};

use super::BlockPayload;
use crate::{rpcs::docs::DocExample, types::DeployHashWithApprovals};

static FINALIZED_BLOCK: Lazy<FinalizedBlock> = Lazy::new(|| {
    let transfer_hashes = vec![*Deploy::doc_example().hash()];
    let random_bit = true;
    let timestamp = *Timestamp::doc_example();
    let secret_key = SecretKey::example();
    let public_key = PublicKey::from(secret_key);
    let validator_set = {
        let mut validator_set = BTreeSet::new();
        validator_set.insert(PublicKey::from(
            &SecretKey::ed25519_from_bytes([5u8; SecretKey::ED25519_LENGTH]).unwrap(),
        ));
        validator_set.insert(PublicKey::from(
            &SecretKey::ed25519_from_bytes([7u8; SecretKey::ED25519_LENGTH]).unwrap(),
        ));
        validator_set
    };
    let rewarded_signatures =
        RewardedSignatures::new(vec![SingleBlockRewardedSignatures::from_validator_set(
            &validator_set,
            &validator_set,
        )]);
    let block_payload = BlockPayload::new(
        vec![],
        transfer_hashes
            .into_iter()
            .map(|hash| {
                let approval = DeployApproval::create(&hash, secret_key);
                let mut approvals = BTreeSet::new();
                approvals.insert(approval);
                DeployHashWithApprovals::new(hash, approvals)
            })
            .collect(),
        vec![],
        rewarded_signatures,
        random_bit,
    );
    let era_report = Some(InternalEraReport::doc_example().clone());
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

static INTERNAL_ERA_REPORT: Lazy<InternalEraReport> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let equivocators = vec![public_key_1];

    let secret_key_3 = SecretKey::ed25519_from_bytes([2; 32]).unwrap();
    let public_key_3 = PublicKey::from(&secret_key_3);
    let inactive_validators = vec![public_key_3];

    InternalEraReport {
        equivocators,
        inactive_validators,
    }
});

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub(crate) deploy_hashes: Vec<DeployHash>,
    pub(crate) transfer_hashes: Vec<DeployHash>,
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
            deploy_hashes: block_payload.deploy_hashes().cloned().collect(),
            transfer_hashes: block_payload.transfer_hashes().cloned().collect(),
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
        let rewarded_signatures = Default::default();
        let random_bit = rng.gen();
        let block_payload =
            BlockPayload::new(deploys, vec![], vec![], rewarded_signatures, random_bit);

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

impl DocExample for FinalizedBlock {
    fn doc_example() -> &'static Self {
        &FINALIZED_BLOCK
    }
}

impl DocExample for InternalEraReport {
    fn doc_example() -> &'static Self {
        &INTERNAL_ERA_REPORT
    }
}

impl From<BlockV2> for FinalizedBlock {
    fn from(block: BlockV2) -> Self {
        FinalizedBlock {
            deploy_hashes: block.deploy_hashes().to_vec(),
            transfer_hashes: block.transfer_hashes().to_vec(),
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
