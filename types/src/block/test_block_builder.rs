use std::iter;

use rand::Rng;

use crate::testing::TestRng;

use crate::{
    system::auction::ValidatorWeights, BlockHash, BlockV1, BlockV2, Deploy, DeployHash, Digest,
    EraEnd, EraId, EraReport, ProtocolVersion, PublicKey, Timestamp, U512,
};

/// A helper to build the blocks with various properties required for tests.
pub struct TestBlockBuilder {
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

struct BlockParameters {
    parent_hash: BlockHash,
    parent_seed: Digest,
    state_root_hash: Digest,
    random_bit: bool,
    era_end: Option<EraEnd>,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    protocol_version: ProtocolVersion,
    proposer: PublicKey,
    deploy_hashes: Vec<DeployHash>,
    transfer_hashes: Vec<DeployHash>,
}

impl Default for TestBlockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestBlockBuilder {
    /// Creates new `TestBlockBuilder`.
    pub fn new() -> Self {
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

    /// Sets the parent hash for the block.
    pub fn parent_hash(mut self, parent_hash: BlockHash) -> Self {
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

    /// Sets the era for the block
    pub fn era(self, era: impl Into<EraId>) -> Self {
        Self {
            era: Some(era.into()),
            ..self
        }
    }

    /// Sets the height for the block.
    pub fn height(mut self, height: u64) -> Self {
        self.height = Some(height);
        self
    }

    /// Sets the protocol version for the block.
    pub fn protocol_version(self, protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            ..self
        }
    }

    /// Associates the given deploys with the created block.
    pub fn deploys<'a, I: IntoIterator<Item = &'a Deploy>>(mut self, deploys_iter: I) -> Self {
        self.deploys = deploys_iter.into_iter().cloned().collect();
        self
    }

    /// Associates a number of random deploys with the created block.
    pub fn random_deploys(mut self, count: usize, rng: &mut TestRng) -> Self {
        self.deploys = iter::repeat(())
            .take(count)
            .map(|_| Deploy::random(rng))
            .collect();
        self
    }

    /// Allows setting the created block to be switch block or not.
    pub fn switch_block(mut self, is_switch: bool) -> Self {
        self.is_switch = Some(is_switch);
        self
    }

    /// Sets the validator weights for the block.
    pub fn validator_weights(self, validator_weights: ValidatorWeights) -> Self {
        Self {
            validator_weights: Some(validator_weights),
            ..self
        }
    }

    fn generate_block_params(&self, rng: &mut TestRng) -> BlockParameters {
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

        let validator_weights = self.validator_weights.clone();
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

        BlockParameters {
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
        }
    }

    /// Builds a block.
    pub fn build(self, rng: &mut TestRng) -> BlockV2 {
        let BlockParameters {
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
        } = self.generate_block_params(rng);

        BlockV2::new(
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

    /// Builds a block that is invalid.
    pub fn build_invalid(self, rng: &mut TestRng) -> BlockV2 {
        self.build(rng).make_invalid(rng)
    }
}

/// Utility trait that supports building test objects from `TestBlockBuilder` instance.
pub trait FromTestBlockBuilder {
    /// Builds `Self` from the given `TestBlockBuilder`.
    fn build_for_test(builder: TestBlockBuilder, rng: &mut TestRng) -> Self;
}

impl FromTestBlockBuilder for BlockV1 {
    fn build_for_test(builder: TestBlockBuilder, rng: &mut TestRng) -> Self {
        let BlockParameters {
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
        } = builder.generate_block_params(rng);

        Self::new(
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
}

impl FromTestBlockBuilder for BlockV2 {
    fn build_for_test(builder: TestBlockBuilder, rng: &mut TestRng) -> Self {
        let BlockParameters {
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
        } = builder.generate_block_params(rng);

        Self::new(
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
}
