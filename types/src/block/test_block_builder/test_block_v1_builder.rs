use std::iter;

use rand::Rng;

use crate::{testing::TestRng, Block, EraEndV1};

use crate::{
    system::auction::ValidatorWeights, BlockHash, BlockV1, Deploy, Digest, EraId, EraReport,
    ProtocolVersion, PublicKey, Timestamp, U512,
};

/// A helper to build the blocks with various properties required for tests.
pub struct TestBlockV1Builder {
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

impl Default for TestBlockV1Builder {
    fn default() -> Self {
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
}

impl TestBlockV1Builder {
    /// Creates new `TestBlockBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the parent hash for the block.
    pub fn parent_hash(self, parent_hash: BlockHash) -> Self {
        Self {
            parent_hash: Some(parent_hash),
            ..self
        }
    }

    /// Sets the state root hash for the block.
    pub fn state_root_hash(self, state_root_hash: Digest) -> Self {
        Self {
            state_root_hash: Some(state_root_hash),
            ..self
        }
    }

    /// Sets the timestamp for the block.
    pub fn timestamp(self, timestamp: Timestamp) -> Self {
        Self {
            timestamp: Some(timestamp),
            ..self
        }
    }

    /// Sets the era for the block
    pub fn era(self, era: impl Into<EraId>) -> Self {
        Self {
            era: Some(era.into()),
            ..self
        }
    }

    /// Sets the height for the block.
    pub fn height(self, height: u64) -> Self {
        Self {
            height: Some(height),
            ..self
        }
    }

    /// Sets the protocol version for the block.
    pub fn protocol_version(self, protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            ..self
        }
    }

    /// Associates the given deploys with the created block.
    pub fn deploys<'a, I: IntoIterator<Item = &'a Deploy>>(self, deploys_iter: I) -> Self {
        Self {
            deploys: deploys_iter.into_iter().cloned().collect(),
            ..self
        }
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
    pub fn switch_block(self, is_switch: bool) -> Self {
        Self {
            is_switch: Some(is_switch),
            ..self
        }
    }

    /// Sets the validator weights for the block.
    pub fn validator_weights(self, validator_weights: ValidatorWeights) -> Self {
        Self {
            validator_weights: Some(validator_weights),
            ..self
        }
    }

    /// Builds the block.
    pub fn build(self, rng: &mut TestRng) -> BlockV1 {
        let Self {
            parent_hash,
            state_root_hash,
            timestamp,
            era,
            height,
            protocol_version,
            deploys,
            is_switch,
            validator_weights,
        } = self;

        let parent_hash = parent_hash.unwrap_or_else(|| BlockHash::new(rng.gen()));
        let parent_seed = Digest::random(rng);
        let state_root_hash = state_root_hash.unwrap_or_else(|| rng.gen());
        let random_bit = rng.gen();
        let is_switch = is_switch.unwrap_or_else(|| rng.gen_bool(0.1));
        let era_end = is_switch.then(|| {
            let next_era_validator_weights = validator_weights.unwrap_or_else(|| {
                (1..6)
                    .map(|i| (PublicKey::random(rng), U512::from(i)))
                    .take(6)
                    .collect()
            });
            EraEndV1::new(EraReport::random(rng), next_era_validator_weights)
        });
        let timestamp = timestamp.unwrap_or_else(Timestamp::now);
        let era_id = era.unwrap_or(EraId::random(rng));
        let height = height.unwrap_or_else(|| era_id.value() * 10 + rng.gen_range(0..10));
        let proposer = PublicKey::random(rng);
        let deploy_hashes = deploys.iter().map(|deploy| *deploy.hash()).collect();
        let transfer_hashes = vec![];

        BlockV1::new(
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

    /// Builds the block as a versioned block.
    pub fn build_versioned(self, rng: &mut TestRng) -> Block {
        self.build(rng).into()
    }
}
