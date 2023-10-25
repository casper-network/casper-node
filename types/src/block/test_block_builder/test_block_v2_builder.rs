use std::iter;

use alloc::collections::BTreeMap;
use rand::Rng;

use crate::{testing::TestRng, Block, EraEndV2, RewardedSignatures};

use crate::{
    system::auction::ValidatorWeights, BlockHash, BlockV2, Deploy, Digest, EraId, ProtocolVersion,
    PublicKey, Timestamp, U512,
};

/// A helper to build the blocks with various properties required for tests.
pub struct TestBlockV2Builder {
    parent_hash: Option<BlockHash>,
    state_root_hash: Option<Digest>,
    timestamp: Option<Timestamp>,
    era: Option<EraId>,
    height: Option<u64>,
    protocol_version: ProtocolVersion,
    deploys: Vec<Deploy>,
    is_switch: Option<bool>,
    validator_weights: Option<ValidatorWeights>,
    rewarded_signatures: Option<RewardedSignatures>,
}

impl Default for TestBlockV2Builder {
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
            rewarded_signatures: None,
        }
    }
}

impl TestBlockV2Builder {
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

    /// Sets the height for the block.
    pub fn rewarded_signatures(self, rewarded_signatures: RewardedSignatures) -> Self {
        Self {
            rewarded_signatures: Some(rewarded_signatures),
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
    pub fn build(self, rng: &mut TestRng) -> BlockV2 {
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
            rewarded_signatures,
        } = self;

        let parent_hash = parent_hash.unwrap_or_else(|| BlockHash::new(rng.gen()));
        let parent_seed = Digest::random(rng);
        let state_root_hash = state_root_hash.unwrap_or_else(|| rng.gen());
        let random_bit = rng.gen();
        let is_switch = is_switch.unwrap_or_else(|| rng.gen_bool(0.1));
        let era_end = is_switch.then(|| gen_era_end_v2(rng, validator_weights));
        let timestamp = timestamp.unwrap_or_else(Timestamp::now);
        let era_id = era.unwrap_or(EraId::random(rng));
        let height = height.unwrap_or_else(|| era_id.value() * 10 + rng.gen_range(0..10));
        let protocol_version = protocol_version;
        let proposer = PublicKey::random(rng);
        let deploy_hashes = deploys.iter().map(|deploy| *deploy.hash()).collect();
        let transfer_hashes = vec![];
        let rewarded_signatures = rewarded_signatures.unwrap_or_default();

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
            rewarded_signatures,
        )
    }

    /// Builds the block as a versioned block.
    pub fn build_versioned(self, rng: &mut TestRng) -> Block {
        self.build(rng).into()
    }

    /// Builds a block that is invalid.
    pub fn build_invalid(self, rng: &mut TestRng) -> BlockV2 {
        self.build(rng).make_invalid(rng)
    }
}

fn gen_era_end_v2(
    rng: &mut TestRng,
    validator_weights: Option<BTreeMap<PublicKey, U512>>,
) -> EraEndV2 {
    let equivocators_count = rng.gen_range(0..5);
    let rewards_count = rng.gen_range(0..5);
    let inactive_count = rng.gen_range(0..5);
    let next_era_validator_weights = validator_weights.unwrap_or_else(|| {
        (1..6)
            .map(|i| (PublicKey::random(rng), U512::from(i)))
            .take(6)
            .collect()
    });
    let equivocators = iter::repeat_with(|| PublicKey::random(rng))
        .take(equivocators_count)
        .collect();
    let rewards = iter::repeat_with(|| {
        let pub_key = PublicKey::random(rng);
        let reward = rng.gen_range(1..=1_000_000_000 + 1);
        (pub_key, U512::from(reward))
    })
    .take(rewards_count)
    .collect();
    let inactive_validators = iter::repeat_with(|| PublicKey::random(rng))
        .take(inactive_count)
        .collect();

    EraEndV2::new(
        equivocators,
        inactive_validators,
        next_era_validator_weights,
        rewards,
    )
}
