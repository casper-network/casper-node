use std::cmp::{max, min};

use datasize::DataSize;
use tracing::error;

use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::consensus::{
        highway_core::{
            finality_detector::{
                assigned_weight_and_latest_unit, find_max_quora, round_participation,
                RoundParticipation,
            },
            state, State,
        },
        traits::Context,
        utils::ValidatorIndex,
    },
    types::chainspec::PerformanceMeterConfig,
};

/// The average max quorum that triggers us to slow down: with this big or smaller average max
/// quorum among some number of past blocks, we increase our round length.
const SLOW_DOWN_THRESHOLD: f64 = 0.8;
/// The average max quorum that triggers us to speed up: with this big or larger average max quorum
/// among some number of past blocks, we decrease our round length.
const ACCELERATION_THRESHOLD: f64 = 0.9;
/// We will try to accelerate (decrease our round length) at least every
/// `MAX_ACCELERATION_PARAMETER` rounds if we have a big enough average max quorum.
const MAX_ACCELERATION_PARAMETER: u64 = 20;

#[derive(DataSize, Debug, Clone)]
pub(crate) enum PerformanceMeter {
    Inactive { config: PerformanceMeterConfig },
    Active(ActivePerformanceMeter),
}

#[derive(DataSize, Debug, Clone)]
pub(crate) struct ActivePerformanceMeter {
    our_vid: ValidatorIndex,
    min_round_len: TimeDiff,
    max_round_len: TimeDiff,
    current_round_len: TimeDiff,
    acceleration_parameter: u64,
    last_exponent_change_round_id: Timestamp,
    config: PerformanceMeterConfig,
}

impl PerformanceMeter {
    /// Creates a new inactive instance of the PerformanceMeter.
    pub fn new_inactive(config: PerformanceMeterConfig) -> Self {
        Self::Inactive { config }
    }

    /// Activates this PerformanceMeter.
    pub fn activate(
        &mut self,
        our_vid: ValidatorIndex,
        round_len: TimeDiff,
        min_round_len: TimeDiff,
        max_round_len: TimeDiff,
        min_era_height: u64,
        timestamp: Timestamp,
    ) {
        match *self {
            Self::Active(_) => (),
            Self::Inactive { config } => {
                let current_round_id = state::round_id(timestamp, round_len);
                *self = Self::Active(ActivePerformanceMeter {
                    our_vid,
                    min_round_len,
                    max_round_len,
                    current_round_len: round_len,
                    acceleration_parameter: min(MAX_ACCELERATION_PARAMETER, min_era_height / 2),
                    last_exponent_change_round_id: current_round_id,
                    config,
                })
            }
        }
    }

    /// Deactivates this PerformanceMeter.
    pub fn deactivate(&mut self) {
        match self {
            Self::Inactive { .. } => (),
            Self::Active(apm) => {
                *self = Self::Inactive { config: apm.config };
            }
        }
    }

    /// Returns an instance of the `PerformanceMeter` for the next era.
    pub fn next_era_perf_meter(&self) -> Self {
        let config = match self {
            Self::Inactive { config } => *config,
            Self::Active(apm) => apm.config,
        };
        // Deliberately start the next era with an inactive meter: even if we were a validator in
        // the current era, we may cease to be one in the next one, or our validator index might
        // change - if we are still a validator in the next era, `activate` will be called, anyway.
        Self::Inactive { config }
    }

    /// Returns the current round length, if active, and `None` otherwise.
    pub fn current_round_len(&self) -> Option<TimeDiff> {
        match self {
            Self::Inactive { .. } => None,
            Self::Active(apm) => Some(apm.current_round_len),
        }
    }

    /// Whenever a block is proposed in a round and nodes cast their confirmation and witness
    /// units, a "max quorum" can be calculated for each node - the maximum quorum for which there
    /// exists a level-1 summit within the round, containing the particular node.
    /// This function calculates the average max quorum for this node out of
    /// `config.blocks_to_consider` most recent ancestor proposals of the current fork choice.
    /// If this average max quorum is below a certain threshold, a higher round length will be
    /// returned.
    /// If it is above a certain threshold, and the current round ID is divisible by a certain
    /// number, a lower round length is returned.
    pub fn calculate_new_length<C: Context>(&mut self, state: &State<C>) -> Option<TimeDiff> {
        let apm = match self {
            Self::Inactive { .. } => {
                return None;
            }
            Self::Active(apm) => apm,
        };

        let panorama = state.panorama();
        let latest_block = match state.fork_choice(panorama) {
            Some(block) => block,
            None => {
                // we have no blocks to check - just return the current setting
                return Some(apm.current_round_len);
            }
        };

        let blocks_to_check = state.ancestor_hashes(latest_block);

        let max_quora: Vec<_> = blocks_to_check
            .take_while(|block| state.unit(block).round_id() >= apm.last_exponent_change_round_id)
            .filter_map(|block| {
                let round_id = state.unit(block).round_id();
                (!matches!(
                    round_participation(state, &panorama[apm.our_vid], round_id),
                    RoundParticipation::Unassigned
                ))
                .then(|| {
                    let (assigned_weight, latest_units) =
                        assigned_weight_and_latest_unit(state, panorama, round_id);
                    let max_quorum = find_max_quora(state, block, &latest_units)
                        .get(apm.our_vid)
                        .copied()
                        .unwrap_or(0u64.into());
                    max_quorum.0 as f64 / assigned_weight.0 as f64
                })
            })
            .take(apm.config.blocks_to_consider as usize)
            .collect();

        if max_quora.len() < apm.config.blocks_to_consider as usize {
            return Some(apm.current_round_len);
        }

        let avg_max_quorum = max_quora.iter().sum::<f64>() / max_quora.len() as f64;

        let current_round_id = state.unit(latest_block).round_id();
        let current_round_index = round_index(current_round_id, apm.current_round_len);

        #[allow(clippy::arithmetic_side_effects)]
        if avg_max_quorum < SLOW_DOWN_THRESHOLD {
            let new_round_len = min(apm.current_round_len * 2, apm.max_round_len);
            if new_round_len != apm.current_round_len {
                apm.current_round_len = new_round_len;
                apm.last_exponent_change_round_id = current_round_id;
            }
        } else if avg_max_quorum > ACCELERATION_THRESHOLD
            && current_round_index % apm.acceleration_parameter == 0
        {
            let new_round_len = max(apm.current_round_len / 2, apm.min_round_len);
            if new_round_len != apm.current_round_len {
                apm.current_round_len = new_round_len;
                apm.last_exponent_change_round_id = current_round_id;
            }
        }

        Some(apm.current_round_len)
    }
}

/// Returns the round index `i`, if `r_id` is the ID of the `i`-th round after the epoch.
#[allow(clippy::arithmetic_side_effects)] // Checking for division by 0.
fn round_index(r_id: Timestamp, round_len: TimeDiff) -> u64 {
    if round_len.millis() == 0 {
        error!("called round_index with round_len 0.");
        return r_id.millis();
    }
    r_id.millis() / round_len.millis()
}
