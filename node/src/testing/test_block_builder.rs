use std::collections::BTreeMap;

use casper_hashing::Digest;
use casper_types::{testing::TestRng, EraId, ProtocolVersion, PublicKey, Timestamp, U512};
use rand::Rng;

use crate::types::{Block, BlockHash, Deploy, FinalizedBlock};

const MAX_ERA_FOR_RANDOM_BLOCK: u64 = 6;

pub(crate) struct TestBlockBuilder {
    era: Option<u64>,
    height: Option<u64>,
    protocol_version: ProtocolVersion,
    is_switch: Option<bool>,
    deploys: Vec<Deploy>,
    state_root_hash: Option<Digest>,
    parent_hash: Option<BlockHash>,
}

impl TestBlockBuilder {
    pub(crate) fn new() -> Self {
        Self {
            era: None,
            height: None,
            protocol_version: ProtocolVersion::V1_0_0,
            is_switch: None,
            deploys: Vec::new(),
            state_root_hash: None,
            parent_hash: None,
        }
    }

    pub(crate) fn era(mut self, era: u64) -> Self {
        self.era = Some(era);
        self
    }

    pub(crate) fn height(mut self, height: u64) -> Self {
        self.height = Some(height);
        self
    }

    pub(crate) fn switch_block(mut self, is_switch: bool) -> Self {
        self.is_switch = Some(is_switch);
        self
    }

    pub(crate) fn state_root_hash(mut self, state_root_hash: Digest) -> Self {
        self.state_root_hash = Some(state_root_hash);
        self
    }

    pub(crate) fn parent_hash(mut self, parent_hash: BlockHash) -> Self {
        self.parent_hash = Some(parent_hash);
        self
    }

    pub(crate) fn deploys<'a, I: IntoIterator<Item = &'a Deploy>>(
        mut self,
        deploys_iter: I,
    ) -> Self {
        self.deploys = deploys_iter.into_iter().cloned().collect();
        self
    }

    pub(crate) fn build(self, rng: &mut TestRng) -> Block {
        let state_root_hash = if let Some(root_hash) = self.state_root_hash {
            root_hash
        } else {
            rng.gen::<[u8; Digest::LENGTH]>().into()
        };

        let era = if let Some(era) = self.era {
            era
        } else {
            rng.gen_range(0..MAX_ERA_FOR_RANDOM_BLOCK)
        };

        let height = if let Some(height) = self.height {
            height
        } else {
            era * 10 + rng.gen_range(0..10)
        };

        let is_switch = if let Some(is_switch) = self.is_switch {
            is_switch
        } else {
            rng.gen_bool(0.1)
        };

        let parent_hash = if let Some(parent_hash) = self.parent_hash {
            parent_hash
        } else {
            BlockHash::new(rng.gen::<[u8; Digest::LENGTH]>().into())
        };

        let finalized_block = FinalizedBlock::random_with_specifics(
            rng,
            EraId::from(era),
            height,
            is_switch,
            Timestamp::now(),
            self.deploys.iter(),
        );
        let parent_seed = rng.gen::<[u8; Digest::LENGTH]>().into();
        let next_era_validator_weights = finalized_block
            .era_report()
            .map(|_| BTreeMap::<PublicKey, U512>::default());

        Block::new(
            parent_hash,
            parent_seed,
            state_root_hash,
            finalized_block,
            next_era_validator_weights,
            self.protocol_version,
        )
        .expect("Could not create random block with specifics")
    }
}
