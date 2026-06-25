use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use num::Zero;
use num_rational::Ratio;
use num_traits::One;

use casper_storage::{
    data_access_layer::{TotalSupplyRequest, TotalSupplyResult},
    global_state::state::StateProvider,
};
use casper_types::{
    Block, ConsensusProtocolName, EraId, ProtocolVersion, PublicKey, Rewards, TimeDiff, U512,
};

use crate::{
    failpoints::FailpointActivation,
    reactor::{
        main_reactor::tests::{
            configs_override::ConfigsOverride, fixture::TestFixture, initial_stakes::InitialStakes,
            switch_blocks::SwitchBlocks, ERA_THREE, ERA_TWO,
        },
        Reactor,
    },
};

// Fundamental network parameters that are not critical for assessing reward calculation correctness
const STAKE: u128 = 1000000000;
const PRIME_STAKES: [u128; 5] = [106907, 106921, 106937, 106949, 106957];
const ERA_COUNT: u64 = 3;
const ERA_DURATION: u64 = 5000;
//milliseconds
const MIN_HEIGHT: u64 = 4;
const BLOCK_TIME: u64 = 1750;
//milliseconds
const TIME_OUT: u64 = 900;
//seconds
const SEIGNIORAGE: (u64, u64) = (1u64, 100u64);
const REPRESENTATIVE_NODE_INDEX: usize = 0;
// Parameters we generally want to vary
const CONSENSUS_ZUG: ConsensusProtocolName = ConsensusProtocolName::Zug;
const CONSENSUS_HIGHWAY: ConsensusProtocolName = ConsensusProtocolName::Highway;
const FINDERS_FEE_ZERO: (u64, u64) = (0u64, 1u64);
const FINDERS_FEE_HALF: (u64, u64) = (1u64, 2u64);
//const FINDERS_FEE_ONE: (u64, u64) = (1u64, 1u64);
const FINALITY_SIG_PROP_ZERO: (u64, u64) = (0u64, 1u64);
const FINALITY_SIG_PROP_HALF: (u64, u64) = (1u64, 2u64);
const FINALITY_SIG_PROP_ONE: (u64, u64) = (1u64, 1u64);
const FILTERED_NODES_INDICES: &[usize] = &[3, 4];
const FINALITY_SIG_LOOKBACK: u64 = 3;

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras() {
    run_rewards_network_scenario(
        PRIME_STAKES,
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras_no_lookback() {
    run_rewards_network_scenario(
        PRIME_STAKES,
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_no_finality_small_nominal_five_eras() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ZERO.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_HALF.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_HALF.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_five_eras() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_zero_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_all_finality_zero_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_HIGHWAY,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_no_finality() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_HIGHWAY,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ZERO.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
async fn rewards_are_calculated() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_era_height: 3,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;
    fixture
        .run_until_consensus_in_era(ERA_THREE, Duration::from_secs(150))
        .await;

    let switch_block = fixture.switch_block(ERA_TWO);

    for reward in switch_block
        .era_end()
        .unwrap()
        .rewards()
        .values()
        .map(|amounts| {
            amounts
                .iter()
                .fold(U512::zero(), |acc, amount| *amount + acc)
        })
    {
        assert_ne!(reward, U512::zero());
    }
}

async fn run_rewards_network_scenario(
    initial_stakes: impl Into<Vec<u128>>,
    era_count: u64,
    time_out: u64, //seconds
    representative_node_index: usize,
    filtered_nodes_indices: &[usize],
    spec_override: ConfigsOverride,
) {
    trait AsU512Ext {
        fn into_u512(self) -> Ratio<U512>;
    }
    impl AsU512Ext for Ratio<u64> {
        fn into_u512(self) -> Ratio<U512> {
            Ratio::new(U512::from(*self.numer()), U512::from(*self.denom()))
        }
    }

    let initial_stakes = initial_stakes.into();

    // Instantiate the chain
    let mut fixture =
        TestFixture::new(InitialStakes::FromVec(initial_stakes), Some(spec_override)).await;

    for i in filtered_nodes_indices {
        let filtered_node = fixture.network.runners_mut().nth(*i).unwrap();
        filtered_node
            .reactor_mut()
            .inner_mut()
            .activate_failpoint(&FailpointActivation::new("finality_signature_creation"));
    }

    // Run the network for a specified number of eras
    let timeout = Duration::from_secs(time_out);
    fixture
        .run_until_stored_switch_block_header(EraId::new(era_count - 1), timeout)
        .await;

    // DATA COLLECTION
    // Get the switch blocks and bid structs first
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), era_count);

    // Representative node
    // (this test should normally run a network at nominal performance with identical nodes)
    let representative_node = fixture
        .network
        .nodes()
        .values()
        .nth(representative_node_index)
        .unwrap();
    let representative_storage = &representative_node.main_reactor().storage;
    let representative_runtime = &representative_node.main_reactor().contract_runtime;

    // Recover highest completed block height
    let highest_completed_height = representative_storage
        .highest_complete_block_height()
        .expect("missing highest completed block");

    // Get all the blocks
    let blocks: Vec<Block> = (0..highest_completed_height + 1)
        .map(|i| {
            representative_storage
                .read_block_by_height(i)
                .expect("block not found")
        })
        .collect();

    let protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    // Get total supply history
    let total_supply: Vec<U512> = (0..highest_completed_height + 1)
        .map(|height: u64| {
            let state_hash = *representative_storage
                .read_block_header_by_height(height, true)
                .expect("failure to read block header")
                .unwrap()
                .state_root_hash();
            let total_supply_req = TotalSupplyRequest::new(state_hash, protocol_version);
            let result = representative_runtime
                .data_access_layer()
                .total_supply(total_supply_req);

            if let TotalSupplyResult::Success { total_supply } = result {
                total_supply
            } else {
                panic!("expected success, not: {:?}", result);
            }
        })
        .collect();

    // Tiny helper function
    #[inline]
    fn add_to_rewards(
        recipient: PublicKey,
        era: EraId,
        reward: Ratio<U512>,
        rewards: &mut BTreeMap<PublicKey, BTreeMap<EraId, Ratio<U512>>>,
    ) {
        match rewards.get_mut(&recipient) {
            Some(map) => {
                *map.entry(era).or_insert(Ratio::zero()) += reward;
            }
            None => {
                let mut map = BTreeMap::new();
                map.insert(era, reward);
                rewards.insert(recipient, map);
            }
        }
    }

    let mut recomputed_total_supply = BTreeMap::new();
    recomputed_total_supply.insert(0, Ratio::from(total_supply[0]));
    let recomputed_rewards: BTreeMap<_, _> = switch_blocks
        .headers
        .iter()
        .enumerate()
        .map(|(i, switch_block)| {
            if switch_block.is_genesis() || switch_block.height() > highest_completed_height {
                return (i, BTreeMap::new());
            }
            let mut recomputed_era_rewards = BTreeMap::new();
            if !switch_block.is_genesis() {
                let supply_carryover = recomputed_total_supply
                    .get(&(i - 1))
                    .copied()
                    .expect("expected prior recomputed supply value");
                recomputed_total_supply.insert(i, supply_carryover);
            }

            // It's not a genesis block, so we know there's something with a lower era id
            let previous_switch_block_height = switch_blocks.headers[i - 1].height();
            let current_era_slated_weights = match switch_blocks.headers[i - 1].clone_era_end() {
                Some(era_report) => era_report.next_era_validator_weights().clone(),
                _ => panic!("unexpectedly absent era report"),
            };
            let total_current_era_weights = current_era_slated_weights
                .iter()
                .fold(U512::zero(), move |acc, s| acc + s.1);
            let weights_block_idx = if switch_blocks.headers[i - 1].is_genesis() {
                i - 1
            } else {
                i - 2
            };
            let (previous_era_slated_weights, total_previous_era_weights) =
                match switch_blocks.headers[weights_block_idx].clone_era_end() {
                    Some(era_report) => {
                        let next_weights = era_report.next_era_validator_weights().clone();
                        let total_next_weights = next_weights
                            .iter()
                            .fold(U512::zero(), move |acc, s| acc + s.1);
                        (next_weights, total_next_weights)
                    }
                    _ => panic!("unexpectedly absent era report"),
                };

            let rewarded_range =
                previous_switch_block_height as usize + 1..switch_block.height() as usize + 1;
            let rewarded_blocks = &blocks[rewarded_range];
            let block_reward = (Ratio::<U512>::one()
                - fixture
                    .chainspec
                    .core_config
                    .finality_signature_proportion
                    .into_u512())
                * recomputed_total_supply[&(i - 1)]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();
            let signatures_reward = fixture
                .chainspec
                .core_config
                .finality_signature_proportion
                .into_u512()
                * recomputed_total_supply[&(i - 1)]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();
            let previous_signatures_reward_idx = if switch_blocks.headers[i - 1].is_genesis() {
                i - 1
            } else {
                i - 2
            };
            let previous_signatures_reward = fixture
                .chainspec
                .core_config
                .finality_signature_proportion
                .into_u512()
                * recomputed_total_supply[&previous_signatures_reward_idx]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();

            rewarded_blocks.iter().for_each(|block: &Block| {
                // Block production rewards
                let proposer = block.proposer().clone();
                add_to_rewards(
                    proposer.clone(),
                    block.era_id(),
                    block_reward,
                    &mut recomputed_era_rewards,
                );

                // Recover relevant finality signatures
                block.rewarded_signatures().iter().enumerate().for_each(
                    |(offset, signatures_packed)| {
                        if block.height() as usize - offset - 1
                            <= previous_switch_block_height as usize
                        {
                            let rewarded_contributors = signatures_packed.to_validator_set(
                                previous_era_slated_weights
                                    .keys()
                                    .cloned()
                                    .collect::<BTreeSet<PublicKey>>(),
                            );
                            rewarded_contributors.iter().for_each(|contributor| {
                                let contributor_proportion = Ratio::new(
                                    previous_era_slated_weights
                                        .get(contributor)
                                        .copied()
                                        .expect("expected current era validator"),
                                    total_previous_era_weights,
                                );
                                // collection always goes to the era in which the block citing the
                                // reward was created
                                add_to_rewards(
                                    proposer.clone(),
                                    block.era_id(),
                                    fixture.chainspec.core_config.finders_fee.into_u512()
                                        * contributor_proportion
                                        * previous_signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                                add_to_rewards(
                                    contributor.clone(),
                                    switch_blocks.headers[i - 1].era_id(),
                                    (Ratio::<U512>::one()
                                        - fixture.chainspec.core_config.finders_fee.into_u512())
                                        * contributor_proportion
                                        * previous_signatures_reward,
                                    &mut recomputed_era_rewards,
                                )
                            });
                        } else {
                            let rewarded_contributors = signatures_packed.to_validator_set(
                                current_era_slated_weights
                                    .keys()
                                    .cloned()
                                    .collect::<BTreeSet<PublicKey>>(),
                            );
                            rewarded_contributors.iter().for_each(|contributor| {
                                let contributor_proportion = Ratio::new(
                                    *current_era_slated_weights
                                        .get(contributor)
                                        .expect("expected current era validator"),
                                    total_current_era_weights,
                                );
                                add_to_rewards(
                                    proposer.clone(),
                                    block.era_id(),
                                    fixture.chainspec.core_config.finders_fee.into_u512()
                                        * contributor_proportion
                                        * signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                                add_to_rewards(
                                    contributor.clone(),
                                    block.era_id(),
                                    (Ratio::<U512>::one()
                                        - fixture.chainspec.core_config.finders_fee.into_u512())
                                        * contributor_proportion
                                        * signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                            });
                        }
                    },
                );
            });

            // Make sure we round just as we do in the real code, at the end of an era's
            // calculation, right before minting and transferring
            recomputed_era_rewards.iter_mut().for_each(|(_, rewards)| {
                rewards.values_mut().for_each(|amount| {
                    *amount = amount.trunc();
                });
                let truncated_reward = rewards.values().sum::<Ratio<U512>>();
                let era_end_supply = recomputed_total_supply
                    .get_mut(&i)
                    .expect("expected supply at end of era");
                *era_end_supply += truncated_reward;
            });

            (i, recomputed_era_rewards)
        })
        .collect();

    // Recalculated total supply is equal to observed total supply
    switch_blocks.headers.iter().for_each(|header| {
        if header.height() <= highest_completed_height {
            assert_eq!(
                Ratio::from(total_supply[header.height() as usize]),
                *(recomputed_total_supply
                    .get(&(header.era_id().value() as usize))
                    .expect("expected recalculated supply")),
                "total supply does not match at height {}",
                header.height()
            );
        }
    });

    // Recalculated rewards are equal to observed rewards; total supply increase is equal to total
    // rewards;
    recomputed_rewards.iter().for_each(|(era, rewards)| {
        if era > &0 && switch_blocks.headers[*era].height() <= highest_completed_height {
            let observed_total_rewards = match switch_blocks.headers[*era]
                .clone_era_end()
                .expect("expected EraEnd")
                .rewards()
            {
                Rewards::V1(v1_rewards) => v1_rewards
                    .iter()
                    .fold(U512::zero(), |acc, reward| U512::from(*reward.1) + acc),
                Rewards::V2(v2_rewards) => v2_rewards
                    .iter()
                    .flat_map(|(_key, amounts)| amounts)
                    .fold(U512::zero(), |acc, reward| *reward + acc),
            };
            let recomputed_total_rewards: U512 = rewards
                .values()
                .flat_map(|amounts| amounts.values().map(|reward| reward.to_integer()))
                .sum();
            assert_eq!(
                Ratio::from(recomputed_total_rewards),
                Ratio::from(observed_total_rewards),
                "total rewards do not match at era {}\nobserved = {:#?}\nrecomputed = {:#?}",
                era,
                switch_blocks.headers[*era]
                    .clone_era_end()
                    .expect("")
                    .rewards(),
                rewards,
            );
            assert_eq!(
                Ratio::from(recomputed_total_rewards),
                recomputed_total_supply
                    .get(era)
                    .expect("expected recalculated supply")
                    - recomputed_total_supply
                        .get(&(era - 1))
                        .expect("expected recalculated supply"),
                "supply growth does not match rewards at era {}",
                era
            )
        }
    })
}
