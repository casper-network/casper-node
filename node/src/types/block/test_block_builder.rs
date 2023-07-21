use std::{collections::BTreeMap, iter};

use rand::Rng;

use casper_types::{
    testing::TestRng, Block, BlockHash, BlockV1, BlockV2, Deploy, DeployHash, Digest, EraEnd,
    EraId, EraReport, ProtocolVersion, PublicKey, Timestamp, U512,
};

pub(crate) struct TestBlockBuilder {
    parent_hash: Option<BlockHash>,
    state_root_hash: Option<Digest>,
    timestamp: Option<Timestamp>,
    era: Option<u64>,
    height: Option<u64>,
    protocol_version: ProtocolVersion,
    deploys: Vec<Deploy>,
    is_switch: Option<bool>,
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

impl TestBlockBuilder {
    #[allow(unused)]
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
        }
    }

    #[allow(unused)]
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

    #[allow(unused)]
    pub(crate) fn era(mut self, era: u64) -> Self {
        self.era = Some(era);
        self
    }

    #[allow(unused)]
    pub(crate) fn height(mut self, height: u64) -> Self {
        self.height = Some(height);
        self
    }

    #[allow(unused)]
    pub(crate) fn deploys<'a, I: IntoIterator<Item = &'a Deploy>>(
        mut self,
        deploys_iter: I,
    ) -> Self {
        self.deploys = deploys_iter.into_iter().cloned().collect();
        self
    }

    #[allow(unused)]
    pub(crate) fn random_deploys(mut self, count: usize, rng: &mut TestRng) -> Self {
        self.deploys = iter::repeat(())
            .take(count)
            .map(|_| Deploy::random(rng))
            .collect();
        self
    }

    #[allow(unused)]
    pub(crate) fn switch_block(mut self, is_switch: bool) -> Self {
        self.is_switch = Some(is_switch);
        self
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

        let era_end = is_switch.then(|| {
            let mut next_era_validator_weights = BTreeMap::new();
            for i in 1_u64..6 {
                let _ = next_era_validator_weights.insert(PublicKey::random(rng), U512::from(i));
            }
            EraEnd::new(EraReport::random(rng), next_era_validator_weights)
        });

        let timestamp = if let Some(timestamp) = self.timestamp {
            timestamp
        } else {
            Timestamp::now()
        };

        let era_id = if let Some(era) = self.era {
            EraId::new(era)
        } else {
            EraId::random(rng)
        };

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

    pub(crate) fn build(self, rng: &mut TestRng) -> Block {
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
}

pub(crate) trait FromTestBlockBuilder {
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
