use crate::testing::{map, set};
use casper_types::{
    testing::TestRng, AsymmetricType as _, EraId, RewardedSignatures, TestBlockBuilder,
};
use once_cell::sync::Lazy;
use std::{iter, ops::Deref};

use self::constructors::RewardsInfoConstructor;

use super::*;
use convert::ratio;

fn val(n: u8) -> PublicKey {
    let mut buf = [0; 32];
    (0..22).for_each(|i| buf[i] = n);
    PublicKey::ed25519_from_bytes(buf).unwrap()
}

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| val(1));
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| val(2));
static VALIDATOR_3: Lazy<PublicKey> = Lazy::new(|| val(3));
static VALIDATOR_4: Lazy<PublicKey> = Lazy::new(|| val(4));

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

#[test]
fn production_payout_increases_with_the_supply() {
    let rng = &mut TestRng::new();
    let percent_signatures = 40;
    let percent_finders = 20;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 6;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
    };

    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![])
            ]),
            EraId::new(1) => (weights.clone(), era_1_reward_per_round, vec![
                (1, VALIDATOR_1.deref(), vec![]),
                (2, VALIDATOR_2.deref(), vec![]),
                (3, VALIDATOR_3.deref(), vec![]),
            ]),
            EraId::new(2) => (weights, era_2_reward_per_round, vec![
                (4, VALIDATOR_3.deref(), vec![]),
                (5, VALIDATOR_1.deref(), vec![]),
                (6, VALIDATOR_2.deref(), vec![]),
            ]),
        },
    );

    // Era payouts:

    let rewards_for_era_1 =
        rewards_for_era(constructor.for_era(rng, 1), EraId::new(1), &core_config).unwrap();
    let rewards_for_era_2 =
        rewards_for_era(constructor.for_era(rng, 2), EraId::new(2), &core_config).unwrap();

    // Checks:

    for ((recipient_1, amounts_1), (recipient_2, amounts_2)) in
        iter::zip(rewards_for_era_1, rewards_for_era_2)
    {
        let amount_1: U512 = amounts_1.into_iter().sum();
        let amount_2: U512 = amounts_2.into_iter().sum();
        assert_eq!(
            ratio(amount_1),
            ratio(era_1_reward_per_round) * ratio(core_config.production_rewards_proportion())
        );
        assert_eq!(
            ratio(amount_2),
            ratio(era_2_reward_per_round) * ratio(core_config.production_rewards_proportion())
        );
        assert_eq!(recipient_1, recipient_2);
        assert_eq!(amount_1 * 4 / 3, amount_2);
    }
}

#[test]
fn production_payout_depends_on_the_blocks_produced() {
    let rng = &mut TestRng::new();
    let percent_signatures = 33;
    let percent_finders = 20;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 4;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights_1 = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
        VALIDATOR_4.clone() => U512::from(89_u64),
    };

    let weights_2 = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(70_u64),
        VALIDATOR_4.clone() => U512::from(89_u64),
    };

    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights_1.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![])
            ]),
            EraId::new(1) => (weights_1, era_1_reward_per_round, vec![
                (1, VALIDATOR_1.deref(), vec![]),
                (2, VALIDATOR_1.deref(), vec![]),
                (3, VALIDATOR_3.deref(), vec![]),
            ]),
            EraId::new(2) => (weights_2, era_2_reward_per_round, vec![
                (4, VALIDATOR_2.deref(), vec![]),
                (5, VALIDATOR_3.deref(), vec![]),
                (6, VALIDATOR_4.deref(), vec![]),
            ]),
        },
    );

    // Era 1 payouts:

    let rewards =
        rewards_for_era(constructor.for_era(rng, 1), EraId::new(1), &core_config).unwrap();

    assert_eq!(
        rewards,
        map! {
            VALIDATOR_1.deref().clone() => vec![(ratio(2 * era_1_reward_per_round) * ratio(core_config.production_rewards_proportion())).to_integer()],
            VALIDATOR_2.deref().clone() => vec![U512::zero()],
            VALIDATOR_3.deref().clone() => vec![(ratio(era_1_reward_per_round) * ratio(core_config.production_rewards_proportion())).to_integer()],
            VALIDATOR_4.deref().clone() => vec![U512::zero()],
        }
    );

    // Era 2 payouts:

    let rewards =
        rewards_for_era(constructor.for_era(rng, 2), EraId::new(2), &core_config).unwrap();

    assert_eq!(
        rewards,
        map! {
            VALIDATOR_1.deref().clone() => vec![U512::zero()],
            VALIDATOR_2.deref().clone() => vec![(ratio(era_2_reward_per_round) * ratio(core_config.production_rewards_proportion())).to_integer()],
            VALIDATOR_3.deref().clone() => vec![(ratio(era_2_reward_per_round) * ratio(core_config.production_rewards_proportion())).to_integer()],
            VALIDATOR_4.deref().clone() => vec![(ratio(era_2_reward_per_round) * ratio(core_config.production_rewards_proportion())).to_integer()],
        }
    );
}

/// Only production & collection fee.
#[test]
fn all_signatures_rewards_without_contribution_fee() {
    let rng = &mut TestRng::new();
    let percent_signatures = 40;
    let percent_finders = 100;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 4;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
    };

    // Simple scenario: each validators sign the block finality directly (no "lag"):
    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![]) // No reward for genesis
            ]),
            EraId::new(1) => (weights.clone(), era_1_reward_per_round, vec![
                (1, VALIDATOR_1.deref(), vec![set!{}]), // Nobody signed the genesis finality
                (2, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}]),
                (3, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}]),
            ]),
            EraId::new(2) => (weights, era_2_reward_per_round, vec![
                (4, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (5, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (6, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
            ]),
        },
    );

    // Era 1 payouts:

    let rewards_for_era_1 =
        rewards_for_era(constructor.for_era(rng, 1), EraId::new(1), &core_config).unwrap();

    let validator_1_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // No finality signature collected:
        + ratio(0) * ratio(core_config.collection_rewards_proportion())
    } * ratio(era_1_reward_per_round);
    let validator_2_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
    } * ratio(era_1_reward_per_round);
    let validator_3_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
    } * ratio(era_1_reward_per_round);

    assert_eq!(
        map! {
            VALIDATOR_1.clone() => vec![validator_1_expected_payout.to_integer()],
            VALIDATOR_2.clone() => vec![validator_2_expected_payout.to_integer()],
            VALIDATOR_3.clone() => vec![validator_3_expected_payout.to_integer()],
        },
        rewards_for_era_1,
    );

    // Era 2 payouts:

    let rewards_for_era_2 =
        rewards_for_era(constructor.for_era(rng, 2), EraId::new(2), &core_config).unwrap();

    let validator_1_expected_payout = {
        // 1 block produced:
        ratio(1)
            * ratio(era_2_reward_per_round)
            * ratio(core_config.production_rewards_proportion())
        // All finality signature collected (paid out in era 2):
        + ratio(era_1_reward_per_round) * ratio(core_config.collection_rewards_proportion())
    };
    let validator_2_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
    } * ratio(era_2_reward_per_round);
    let validator_3_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
    } * ratio(era_2_reward_per_round);

    assert_eq!(
        map! {
            VALIDATOR_1.clone() => vec![validator_1_expected_payout.to_integer()],
            VALIDATOR_2.clone() => vec![validator_2_expected_payout.to_integer()],
            VALIDATOR_3.clone() => vec![validator_3_expected_payout.to_integer()],
        },
        rewards_for_era_2,
    );
}

/// Only production & contribution fee.
#[test]
fn all_signatures_rewards_without_finder_fee() {
    let rng = &mut TestRng::new();
    let percent_signatures = 40;
    let percent_finders = 0;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 4;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
    };

    // Simple scenario: each validators sign the block finality directly (no "lag"):
    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![]) // No reward for genesis
            ]),
            EraId::new(1) => (weights.clone(), era_1_reward_per_round, vec![
                (1, VALIDATOR_1.deref(), vec![set!{}]), // Nobody signed the genesis finality
                (2, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}]),
                (3, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}]),
            ]),
            EraId::new(2) => (weights, era_2_reward_per_round, vec![
                (4, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (5, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (6, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
            ]),
        },
    );

    // Era 1 payouts:

    let rewards_for_era_1 =
        rewards_for_era(constructor.for_era(rng, 1), EraId::new(1), &core_config).unwrap();

    let validator_1_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_1.deref())
    } * ratio(era_1_reward_per_round);
    let validator_2_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_2.deref())
    } * ratio(era_1_reward_per_round);
    let validator_3_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_3.deref())
    } * ratio(era_1_reward_per_round);

    assert_eq!(
        rewards_for_era_1,
        map! {
            VALIDATOR_1.clone() => vec![validator_1_expected_payout.to_integer()],
            VALIDATOR_2.clone() => vec![validator_2_expected_payout.to_integer()],
            VALIDATOR_3.clone() => vec![validator_3_expected_payout.to_integer()],
        }
    );
}

#[test]
fn all_signatures_rewards() {
    let rng = &mut TestRng::new();
    let percent_signatures = 40;
    let percent_finders = 15;
    let blocks_per_era = 3;
    let signature_rewards_max_delay = 4;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
    };

    // Simple scenario: each validators sign the block finality directly (no "lag"):
    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![]) // No reward for genesis
            ]),
            EraId::new(1) => (weights.clone(), era_1_reward_per_round, vec![
                (1, VALIDATOR_1.deref(), vec![set!{}]), // Nobody signed the genesis finality
                (2, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}]),
                (3, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}]),
            ]),
            EraId::new(2) => (weights, era_2_reward_per_round, vec![
                (4, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (5, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
                (6, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{}, set!{}, set!{}]),
            ]),
        },
    );

    // Era 1 payouts:

    let rewards_for_era_1 =
        rewards_for_era(constructor.for_era(rng, 1), EraId::new(1), &core_config).unwrap();

    let validator_1_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // No finality signature collected:
        + ratio(0) * ratio(core_config.collection_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_1.deref())
    } * ratio(era_1_reward_per_round);
    let validator_2_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_2.deref())
    } * ratio(era_1_reward_per_round);
    let validator_3_expected_payout = {
        // 1 block produced:
        ratio(1) * ratio(core_config.production_rewards_proportion())
        // All finality signatures collected:
        + ratio(core_config.collection_rewards_proportion())
        // 2 finality signed:
        + ratio(2) * ratio(core_config.contribution_rewards_proportion()) * constructor.weight(1, VALIDATOR_3.deref())
    } * ratio(era_1_reward_per_round);

    assert_eq!(
        rewards_for_era_1,
        map! {
            VALIDATOR_1.clone() => vec![validator_1_expected_payout.to_integer()],
            VALIDATOR_2.clone() => vec![validator_2_expected_payout.to_integer()],
            VALIDATOR_3.clone() => vec![validator_3_expected_payout.to_integer()],
        }
    );
}

#[test]
fn mixed_signatures_pattern() {
    let rng = &mut TestRng::new();
    let percent_signatures = 30;
    let percent_finders = 27;
    let blocks_per_era = 4;
    let signature_rewards_max_delay = 4;
    let core_config = core_config(
        rng,
        percent_signatures,
        percent_finders,
        blocks_per_era,
        signature_rewards_max_delay,
    );

    let production = ratio(core_config.production_rewards_proportion());
    let collection = ratio(core_config.collection_rewards_proportion());
    let contribution = ratio(core_config.contribution_rewards_proportion());

    // Eras info:

    let era_1_reward_per_round = 300;
    let era_2_reward_per_round = 400;

    let weights_1 = map! {
        VALIDATOR_1.clone() => U512::from(100_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(30_u64),
    };

    let weights_2 = map! {
        VALIDATOR_1.clone() => U512::from(93_u64),
        VALIDATOR_2.clone() => U512::from(190_u64),
        VALIDATOR_3.clone() => U512::from(69_u64),
        VALIDATOR_4.clone() => U512::from(212_u64),
    };

    // Complex scenario:
    // - not all validators sign
    // - in era 2, signatures are reported from era 1
    let constructor = RewardsInfoConstructor::new(
        &core_config,
        map! {
            EraId::new(0) => (weights_1.clone(), era_1_reward_per_round, vec![
                (0, VALIDATOR_1.deref(), vec![]) // No reward for genesis
            ]),
            EraId::new(1) => (weights_1, era_1_reward_per_round, vec![
                (1, VALIDATOR_2.deref(), vec![set!{}]), // Nobody signed the genesis finality
                (2, VALIDATOR_2.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_3.clone()}, set!{}]),
                (3, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{VALIDATOR_2.clone()}, set!{}]), // the validator 2 signature is fetched later
                (4, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone()}, set!{}, set!{}]), // validator 3 doesn't sign the block 3
            ]),
            EraId::new(2) => (weights_2, era_2_reward_per_round, vec![
                (5, VALIDATOR_2.deref(), vec![set!{}, set!{}, set!{}, set!{}]),
                (6, VALIDATOR_3.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone(),VALIDATOR_4.clone()}, set!{VALIDATOR_1.clone()}, set!{}, set!{}]),
                (7, VALIDATOR_4.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone(),VALIDATOR_3.clone()}, set!{VALIDATOR_3.clone(),VALIDATOR_4.clone()}, set!{VALIDATOR_3.clone()}, set!{}]),
                (8, VALIDATOR_1.deref(), vec![set!{VALIDATOR_1.clone(),VALIDATOR_2.clone()}, set!{}, set!{}, set!{}]),
            ]),
        },
    );

    // Era 1 payouts:
    {
        let era = EraId::new(1);
        let rewards_for_era_1 =
            rewards_for_era(constructor.for_era(rng, era), era, &core_config).unwrap();

        let validator_1_expected_payout = {
            // 2 blocks produced:
            ratio(2) * production
            // 6 finality signatures collected:
            + collection * (
                ratio(2) * constructor.weight(era, VALIDATOR_1.deref())
                + ratio(3) * constructor.weight(era, VALIDATOR_2.deref())
                + ratio(1) * constructor.weight(era, VALIDATOR_3.deref())
            )
            // 3 finality signed:
            + ratio(3) * contribution * constructor.weight(era, VALIDATOR_1.deref())
        } * ratio(era_1_reward_per_round);
        let validator_2_expected_payout = {
            // 2 blocks produced:
            ratio(2) * production
            // 2 finality signatures collected:
            + collection * (
                ratio(1) * constructor.weight(era, VALIDATOR_1.deref())
                + ratio(0) * constructor.weight(era, VALIDATOR_2.deref())
                + ratio(1) * constructor.weight(era, VALIDATOR_3.deref())
            )
            // 3 finality signed:
            + ratio(3) * contribution * constructor.weight(era, VALIDATOR_2.deref())
        } * ratio(era_1_reward_per_round);
        let validator_3_expected_payout = {
            // No block produced:
            ratio(0) * production
            // No finality signatures collected:
            + ratio(0) * collection
            // 2 finality signed:
            + ratio(2) * contribution * constructor.weight(era, VALIDATOR_3.deref())
        } * ratio(era_1_reward_per_round);

        assert_eq!(
            rewards_for_era_1,
            map! {
                VALIDATOR_1.clone() => vec![validator_1_expected_payout.to_integer()],
                VALIDATOR_2.clone() => vec![validator_2_expected_payout.to_integer()],
                VALIDATOR_3.clone() => vec![validator_3_expected_payout.to_integer()],
            }
        );
    }

    // Era 2 payouts:
    {
        let era = EraId::new(2);
        let rewards_for_era_2 =
            rewards_for_era(constructor.for_era(rng, era), era, &core_config).unwrap();

        let validator_1_expected_payout = vec![
            // 1 block produced:
            (production * ratio(1) * ratio(era_2_reward_per_round)
            // 2 finality signatures collected:
            + collection * {
                ratio(1) * constructor.weight(era, VALIDATOR_1.deref())
                + ratio(1) * constructor.weight(era, VALIDATOR_2.deref())
            } * ratio(era_2_reward_per_round)
            // Finality signed:
            + contribution * {
                // 3 in current era:
                ratio(3) * ratio(era_2_reward_per_round) * constructor.weight(era, VALIDATOR_1.deref())
            }).to_integer(),
            // 1 contributed in previous era:
            (contribution * {
                ratio(1) * ratio(era_1_reward_per_round) * constructor.weight(1, VALIDATOR_1.deref())
            }).to_integer()
        ];

        let validator_2_expected_payout = vec![
            // 1 block produced:
            (ratio(1) * production * ratio(era_2_reward_per_round)
            // No finality signature collected:
            // 3 finality signed:
            + ratio(3) * contribution * ratio(era_2_reward_per_round) * constructor.weight(era, VALIDATOR_2.deref()))
            .to_integer()
        ];

        let validator_3_expected_payout = vec![
            // 1 block produced:
            (ratio(1) * production * ratio(era_2_reward_per_round)
            // 2 finality signatures collected:
            + collection * {
                (
                    ratio(1) * constructor.weight(era, VALIDATOR_1.deref())
                    + ratio(1) * constructor.weight(era, VALIDATOR_2.deref())
                    + ratio(1) * constructor.weight(era, VALIDATOR_3.deref())
                    + ratio(1) * constructor.weight(era, VALIDATOR_4.deref())
                ) * ratio(era_2_reward_per_round)
                // collected one signature from era 1
                + (
                    ratio(1) * constructor.weight(1, VALIDATOR_1.deref())
                ) * ratio(era_1_reward_per_round)
            }
            // Finality signed:
            + contribution * {
                // 3 in current era:
                ratio(3) * ratio(era_2_reward_per_round) * constructor.weight(era, VALIDATOR_3.deref())
            }).to_integer(),
            // for era 1
            (contribution * {
                // 1 in previous era:
                ratio(1) * ratio(era_1_reward_per_round) * constructor.weight(1, VALIDATOR_3.deref())
            }).to_integer()
        ];

        let validator_4_expected_payout = vec![
            // 1 block produced:
            (ratio(1) * production * ratio(era_2_reward_per_round)
            // 6 finality signatures collected:
            + collection * {
                (
                    ratio(1) * constructor.weight(era, VALIDATOR_1.deref())
                    + ratio(1) * constructor.weight(era, VALIDATOR_2.deref())
                    + ratio(2) * constructor.weight(era, VALIDATOR_3.deref())
                    + ratio(1) * constructor.weight(era, VALIDATOR_4.deref())
                ) * ratio(era_2_reward_per_round)
                // collected one signature from era 1
                + (
                    ratio(1) * constructor.weight(1, VALIDATOR_3.deref())
                ) * ratio(era_1_reward_per_round)
            }
            // 3 finality signed:
            + ratio(2) * contribution * ratio(era_2_reward_per_round) * constructor.weight(era, VALIDATOR_4.deref()))
            .to_integer(),
        ];

        assert_eq!(
            rewards_for_era_2,
            map! {
                VALIDATOR_1.clone() => validator_1_expected_payout,
                VALIDATOR_2.clone() => validator_2_expected_payout,
                VALIDATOR_3.clone() => validator_3_expected_payout,
                VALIDATOR_4.clone() => validator_4_expected_payout,
            }
        );
    }
}

mod constructors {
    use casper_types::SingleBlockRewardedSignatures;

    use super::*;
    use std::collections::BTreeSet;

    type Weights = BTreeMap<PublicKey, U512>;
    type RewardPerRound = u64;
    type BlockInfo<'a> = (u64, &'a PublicKey, Vec<BTreeSet<PublicKey>>);

    pub(super) struct RewardsInfoConstructor<'a> {
        signature_rewards_max_delay: u64,
        blocks: BTreeMap<EraId, (Weights, RewardPerRound, Vec<BlockInfo<'a>>)>,
        /// A cache with the validators for each era
        validators: BTreeMap<EraId, BTreeSet<PublicKey>>,
    }

    impl<'a> RewardsInfoConstructor<'a> {
        pub(super) fn new(
            core_config: &'a CoreConfig,
            blocks: BTreeMap<EraId, (Weights, RewardPerRound, Vec<BlockInfo<'a>>)>,
        ) -> Self {
            let validators = blocks
                .iter()
                .map(|(era_id, (weights, _, _))| (*era_id, weights.keys().cloned().collect()))
                .collect();

            Self {
                signature_rewards_max_delay: core_config.signature_rewards_max_delay,
                blocks,
                validators,
            }
        }

        /// Returns the relative weight for a validator.
        pub(super) fn weight(
            &self,
            era_id: impl Into<EraId>,
            validator: &PublicKey,
        ) -> Ratio<U512> {
            let weights = &self.blocks[&era_id.into()].0;
            let total = weights.values().copied().sum();
            let weight = weights[validator];

            Ratio::new(weight, total)
        }

        pub(super) fn for_era(&self, rng: &mut TestRng, era_id: impl Into<EraId>) -> RewardsInfo {
            let era_id = era_id.into();
            let number_blocks = {
                let era_size = self.blocks[&era_id].2.len();
                self.signature_rewards_max_delay as usize + era_size
            };

            let cited_blocks: Vec<_> = self
                .blocks
                .range(EraId::new(0)..=era_id)
                .rev()
                .flat_map(|(era_id, (_, _, blocks))| {
                    let switch_height = blocks.iter().map(|b| b.0).max().unwrap();
                    // Blocks are being read in reverse, era by era, so that we can build only the
                    // latest needed:
                    blocks.clone().into_iter().rev().map(
                        move |(height, proposer, rewarded_signatures)| {
                            let rewarded_signatures = RewardedSignatures::new(
                                rewarded_signatures.into_iter().enumerate().map(
                                    |(height_offset, signing_validators)| {
                                        let height =
                                            height.saturating_sub(height_offset as u64 + 1);
                                        let era_id = self
                                            .blocks
                                            .iter()
                                            .find_map(|(era_id, (_, _, blocks))| {
                                                blocks
                                                    .iter()
                                                    .find(|(h, _, _)| h == &height)
                                                    .map(|_| era_id)
                                            })
                                            .unwrap_or_else(|| {
                                                panic!("height {} must be provided", height)
                                            });
                                        let era_validators = self
                                            .validators
                                            .get(era_id)
                                            .expect("the info for the era to be provided");
                                        SingleBlockRewardedSignatures::from_validator_set(
                                            &signing_validators,
                                            era_validators,
                                        )
                                    },
                                ),
                            );
                            TestBlockBuilder::new()
                                .height(height)
                                .era(*era_id)
                                .proposer(proposer.clone())
                                .rewarded_signatures(rewarded_signatures)
                                .switch_block(height == switch_height)
                        },
                    )
                })
                .map(move |block_builder| CitedBlock::from(Block::from(block_builder.build(rng))))
                .take(number_blocks)
                .collect();
            let cited_blocks: Vec<_> = cited_blocks.into_iter().rev().collect();

            let first_block = cited_blocks.first().expect("at least one cited block");
            assert!(
                cited_blocks.len() >= number_blocks || first_block.is_genesis,
                "Not enough blocks provided"
            );

            let eras_info = self
                .blocks
                .range(first_block.era_id..=era_id)
                .map(|(era_id, (weights, reward_per_round, _))| {
                    (
                        *era_id,
                        EraInfo::new_testing(weights.clone(), ratio(*reward_per_round)),
                    )
                })
                .collect();

            RewardsInfo::new_testing(eras_info, cited_blocks)
        }
    }
}

mod convert {
    use super::*;
    use std::convert::TryFrom;

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

    impl IntoRatioU512 for usize {
        fn into(self) -> Ratio<U512> {
            Ratio::new(U512::from(self), U512::one())
        }
    }

    impl IntoRatioU512 for U512 {
        fn into(self) -> Ratio<U512> {
            Ratio::new(self, U512::one())
        }
    }

    impl IntoRatioU512 for i32 {
        fn into(self) -> Ratio<U512> {
            Ratio::new(U512::from(u32::try_from(self).unwrap()), U512::one())
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
