use crate::testing::map;
use casper_types::{testing::TestRng, EraId, SecretKey, TestBlockBuilder};
use once_cell::sync::Lazy;
use std::iter;

use super::*;
use convert::ratio;

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([5; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([7; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_4: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([10; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

fn core_config(
    rng: &mut TestRng,
    percent_signatures: u64,
    percent_finders: u64,
    minimum_era_height: u64,
    signature_rewards_max_delay: u64,
) -> CoreConfig {
    CoreConfig {
        finality_signature_proportion: Ratio::new(percent_signatures, 100),
        finders_fee: Ratio::new(percent_finders, 100),
        signature_rewards_max_delay,
        minimum_era_height,
        ..CoreConfig::random(rng)
    }
}

fn create_rewards_info(
    rng: &mut TestRng,
    eras_info: BTreeMap<EraId, EraInfo>,
    core_config: &CoreConfig,
) -> RewardsInfo {
    /// Creates blocks for the given era. The proposer is chosen with a round-robin mechanism.
    fn create_blocks_for_era(
        rng: &mut TestRng,
        era_id: EraId,
        range: Range<u64>,
        validators: BTreeMap<PublicKey, U512>,
    ) -> Vec<Block> {
        let switch = range.end;

        range
            .zip(validators.keys().cloned().cycle())
            .map(move |(height, proposer)| {
                TestBlockBuilder::new()
                    .height(height)
                    .era(era_id)
                    .proposer(proposer)
                    .switch_block(height == switch)
                    .build_versioned(rng)
            })
            .collect()
    }

    let step = core_config.minimum_era_height;
    let min_height = {
        let highest_height = step
            * eras_info
                .keys()
                .copied()
                .max()
                .expect("at least one era")
                .value();

        highest_height
            .checked_sub(core_config.signature_rewards_max_delay - 1)
            .unwrap()
    };

    let cited_blocks = eras_info
        .iter()
        .flat_map(move |(era_id, era_info)| {
            if era_id.is_genesis() {
                create_blocks_for_era(rng, *era_id, 0..1, Default::default())
            } else {
                let start = ((era_id.value() - 1) * step + 1).max(min_height);
                let switch = era_id.value() * step;
                create_blocks_for_era(rng, *era_id, start..(switch + 1), era_info.weights.clone())
            }
        })
        .map(Some)
        .collect();

    RewardsInfo::new_testing(eras_info, cited_blocks)
}

#[test]
fn payout_increase_with_the_supply() {
    let rng = &mut TestRng::new();
    let percent_signatures = 40;
    let percent_finders = 20;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 3;
    let era_2_reward_per_round = 300;
    let era_3_reward_per_round = 400;

    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Era 2 payout:

    let era_info_2 = {
        let weights_2 = map! {
            VALIDATOR_1.clone() => U512::from(100_u64),
            VALIDATOR_2.clone() => U512::from(190_u64),
            VALIDATOR_3.clone() => U512::from(30_u64),
        };

        EraInfo::new_testing(weights_2.clone(), ratio(era_2_reward_per_round))
    };

    let rewards_for_era_2 = rewards_for_era(
        create_rewards_info(rng, map! { EraId::new(2) => era_info_2 }, &core_config),
        EraId::new(2),
        &core_config,
    )
    .unwrap();

    // Era 3 payout:

    let era_info_3 = {
        let weights_3 = map! {
            VALIDATOR_1.clone() => U512::from(100_u64),
            VALIDATOR_2.clone() => U512::from(190_u64),
            VALIDATOR_3.clone() => U512::from(30_u64),
        };

        EraInfo::new_testing(weights_3.clone(), ratio(era_3_reward_per_round))
    };

    let rewards_for_era_3 = rewards_for_era(
        create_rewards_info(rng, map! { EraId::new(3) => era_info_3 }, &core_config),
        EraId::new(3),
        &core_config,
    )
    .unwrap();

    // Checks:

    for ((recipient_2, amount_2), (recipient_3, amount_3)) in
        iter::zip(rewards_for_era_2, rewards_for_era_3)
    {
        assert_eq!(
            ratio(amount_2),
            ratio(era_2_reward_per_round) * ratio(core_config.production_rewards_proportion())
        );
        assert_eq!(
            ratio(amount_3),
            ratio(era_3_reward_per_round) * ratio(core_config.production_rewards_proportion())
        );
        assert_eq!(recipient_2, recipient_3);
        assert_eq!(amount_2 * 4 / 3, amount_3);
    }
}

mod convert {
    use super::*;

    pub(super) fn ratio(n: impl IntoRatioU512) -> Ratio<U512> {
        n.into()
    }

    pub(super) trait IntoRatioU512 {
        fn into(self) -> Ratio<U512>;
    }

    impl IntoRatioU512 for u64 {
        fn into(self) -> Ratio<U512> {
            Ratio::new(U512::from(self), U512::one())
        }
    }

    impl IntoRatioU512 for U512 {
        fn into(self) -> Ratio<U512> {
            Ratio::new(self, U512::one())
        }
    }

    impl IntoRatioU512 for Ratio<u64> {
        fn into(self) -> Ratio<U512> {
            Ratio::new(U512::from(*self.numer()), U512::from(*self.denom()))
        }
    }

    impl IntoRatioU512 for Ratio<U512> {
        fn into(self) -> Ratio<U512> {
            self
        }
    }
}
