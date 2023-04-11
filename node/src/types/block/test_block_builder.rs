use std::collections::BTreeMap;

use casper_hashing::Digest;
use casper_types::{
    system::auction::BLOCK_REWARD, testing::TestRng, EraId, ProtocolVersion, PublicKey, SecretKey,
    Timestamp, U512,
};
use rand::Rng;

use crate::{
    components::consensus::EraReport,
    types::{Block, BlockHash, BlockPayload, Deploy, DeployHashWithApprovals, FinalizedBlock},
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

impl TestBlockBuilder {
    #[allow(unused)]
    pub(crate) fn new() -> Self {
        Self {
            era: None,
            height: None,
            protocol_version: ProtocolVersion::V1_0_0,
            is_switch: None,
            deploys: Vec::new(),
            state_root_hash: None,
            parent_hash: None,
            timestamp: None,
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
    pub(crate) fn switch_block(mut self, is_switch: bool) -> Self {
        self.is_switch = Some(is_switch);
        self
    }

    #[allow(unused)]
    pub(crate) fn build(self, rng: &mut TestRng) -> Block {
        let state_root_hash = if let Some(root_hash) = self.state_root_hash {
            root_hash
        } else {
            rng.gen::<[u8; Digest::LENGTH]>().into()
        };

        let era = if let Some(era) = self.era {
            era
        } else {
            rng.gen_range(0..super::MAX_ERA_FOR_RANDOM_BLOCK)
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

        let timestamp = if let Some(timestamp) = self.timestamp {
            timestamp
        } else {
            Timestamp::now()
        };

        let finalized_block = {
            use std::iter;

            let deploy_hashes = self
                .deploys
                .iter()
                .map(DeployHashWithApprovals::from)
                .collect::<Vec<_>>();

            let random_bit = rng.gen();
            let block_payload = BlockPayload::new(deploy_hashes, vec![], vec![], random_bit);

            let era_report = if is_switch {
                let equivocators_count = rng.gen_range(0..5);
                let rewards_count = rng.gen_range(0..5);
                let inactive_count = rng.gen_range(0..5);
                Some(EraReport {
                    equivocators: iter::repeat_with(|| {
                        PublicKey::from(
                            &SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap(),
                        )
                    })
                    .take(equivocators_count)
                    .collect(),
                    rewards: iter::repeat_with(|| {
                        let pub_key = PublicKey::from(
                            &SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap(),
                        );
                        let reward = rng.gen_range(1..(BLOCK_REWARD + 1));
                        (pub_key, reward)
                    })
                    .take(rewards_count)
                    .collect(),
                    inactive_validators: iter::repeat_with(|| {
                        PublicKey::from(
                            &SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap(),
                        )
                    })
                    .take(inactive_count)
                    .collect(),
                })
            } else {
                None
            };
            let secret_key: SecretKey =
                SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();
            let public_key = PublicKey::from(&secret_key);

            FinalizedBlock::new(
                block_payload,
                era_report,
                timestamp,
                EraId::from(era),
                height,
                public_key,
            )
        };

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
