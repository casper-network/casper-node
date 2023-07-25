use std::iter;

use rand::Rng;

use casper_types::{
    system::auction::ValidatorWeights, testing::TestRng, Block, BlockHash, Deploy, Digest, EraEnd,
    EraId, EraReport, ProtocolVersion, PublicKey, Timestamp, U512,
};

pub(crate) struct TestBlockBuilder {
    parent_hash: Option<BlockHash>,
    state_root_hash: Option<Digest>,
    timestamp: Option<Timestamp>,
    era: Option<EraId>,
    height: Option<u64>,
    protocol_version: ProtocolVersion,
    deploys: Vec<Deploy>,
    is_switch: Option<bool>,
    validator_weights: Option<ValidatorWeights>,
}

impl TestBlockBuilder {
    pub(crate) fn new() -> Self {
        Self {
            parent_hash: None,
            state_root_hash: None,
            timestamp: None,
            era: None,
            height: None,
            protocol_version: ProtocolVersion::V1_0_0,
            deploys: Vec::new(),
            is_switch: None,
            validator_weights: None,
        }
    }

    pub(crate) fn parent_hash(mut self, parent_hash: BlockHash) -> Self {
        self.parent_hash = Some(parent_hash);
        self
    }

    #[allow(unused)]
    pub(crate) fn state_root_hash(mut self, state_root_hash: Digest) -> Self {
        self.state_root_hash = Some(state_root_hash);
        self
    }

    #[allow(unused)]
    pub(crate) fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub(crate) fn era(self, era: impl Into<EraId>) -> Self {
        Self {
            era: Some(era.into()),
            ..self
        }
    }

    pub(crate) fn height(mut self, height: u64) -> Self {
        self.height = Some(height);
        self
    }

    pub(crate) fn protocol_version(self, protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            ..self
        }
    }

    pub(crate) fn deploys<'a, I: IntoIterator<Item = &'a Deploy>>(
        mut self,
        deploys_iter: I,
    ) -> Self {
        self.deploys = deploys_iter.into_iter().cloned().collect();
        self
    }

    pub(crate) fn random_deploys(mut self, count: usize, rng: &mut TestRng) -> Self {
        self.deploys = iter::repeat(())
            .take(count)
            .map(|_| Deploy::random(rng))
            .collect();
        self
    }

    pub(crate) fn switch_block(mut self, is_switch: bool) -> Self {
        self.is_switch = Some(is_switch);
        self
    }

    pub(crate) fn validator_weights(self, validator_weights: ValidatorWeights) -> Self {
        Self {
            validator_weights: Some(validator_weights),
            ..self
        }
    }

    pub(crate) fn build(self, rng: &mut TestRng) -> Block {
        let parent_hash = if let Some(parent_hash) = self.parent_hash {
            parent_hash
        } else {
            BlockHash::new(rng.gen::<[u8; Digest::LENGTH]>().into())
        };

        let parent_seed = Digest::random(rng);

        let state_root_hash = if let Some(root_hash) = self.state_root_hash {
            root_hash
        } else {
            rng.gen::<[u8; Digest::LENGTH]>().into()
        };

        let random_bit = rng.gen();

        let is_switch = if let Some(is_switch) = self.is_switch {
            is_switch
        } else {
            rng.gen_bool(0.1)
        };

        let validator_weights = self.validator_weights;
        let era_end = is_switch.then(|| {
            let next_era_validator_weights = validator_weights.unwrap_or_else(|| {
                (1..6)
                    .map(|i| (PublicKey::random(rng), U512::from(i)))
                    .take(6)
                    .collect()
            });
            EraEnd::new(EraReport::random(rng), next_era_validator_weights)
        });

        let timestamp = if let Some(timestamp) = self.timestamp {
            timestamp
        } else {
            Timestamp::now()
        };

        let era_id = self.era.unwrap_or(EraId::random(rng));

        let height = if let Some(height) = self.height {
            height
        } else {
            era_id.value() * 10 + rng.gen_range(0..10)
        };

        let protocol_version = self.protocol_version;

        let proposer = PublicKey::random(rng);

        let deploy_hashes = self.deploys.iter().map(|deploy| *deploy.hash()).collect();

        let transfer_hashes = vec![];

        Block::new(
            parent_hash,
            parent_seed,
            state_root_hash,
            random_bit,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
            proposer,
            deploy_hashes,
            transfer_hashes,
        )
    }

    pub(crate) fn build_invalid(self, rng: &mut TestRng) -> Block {
        self.build(rng).make_invalid(rng)
    }
}
