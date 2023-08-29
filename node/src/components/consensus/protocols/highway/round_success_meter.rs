use std::{cmp::max, collections::VecDeque, mem};

use datasize::DataSize;
use tracing::{error, trace};

use casper_types::{TimeDiff, Timestamp};

use crate::components::consensus::{
    highway_core::{finality_detector::FinalityDetector, state, State},
    traits::Context,
    utils::Weight,
};

pub(crate) mod config;
use config::*;

#[derive(DataSize, Debug, Clone)]
pub(crate) struct RoundSuccessMeter<C>
where
    C: Context,
{
    // store whether a particular round was successful
    // index 0 is the last handled round, 1 is the second-to-last etc.
    rounds: VecDeque<bool>,
    current_round_id: Timestamp,
    proposals: Vec<C::Hash>,
    min_round_len: TimeDiff,
    max_round_len: TimeDiff,
    current_round_len: TimeDiff,
    config: Config,
}

impl<C: Context> RoundSuccessMeter<C> {
    pub fn new(
        round_len: TimeDiff,
        min_round_len: TimeDiff,
        max_round_len: TimeDiff,
        timestamp: Timestamp,
        config: Config,
    ) -> Self {
        let current_round_id = state::round_id(timestamp, round_len);
        Self {
            rounds: VecDeque::with_capacity(config.num_rounds_to_consider as usize),
            current_round_id,
            proposals: Vec::new(),
            min_round_len,
            max_round_len,
            current_round_len: round_len,
            config,
        }
    }

    fn change_length(&mut self, new_len: TimeDiff, timestamp: Timestamp) {
        self.rounds = VecDeque::with_capacity(self.config.num_rounds_to_consider as usize);
        self.current_round_len = new_len;
        self.current_round_id = state::round_id(timestamp, new_len);
        self.proposals = Vec::new();
    }

    fn check_proposals_success(&self, state: &State<C>, proposal_h: &C::Hash) -> bool {
        let total_w = state.total_weight();

        #[allow(clippy::integer_arithmetic)] // FTT is less than 100%, so this can't overflow.
        let finality_detector = FinalityDetector::<C>::new(max(
            Weight(
                (u128::from(total_w) * *self.config.acceleration_ftt.numer() as u128
                    / *self.config.acceleration_ftt.denom() as u128) as u64,
            ),
            Weight(1),
        ));

        // check for the existence of a level-1 summit
        finality_detector.find_summit(1, proposal_h, state) == 1
    }

    /// Registers a proposal within this round - if it's finalized within the round, the round will
    /// be successful.
    pub fn new_proposal(&mut self, proposal_h: C::Hash, timestamp: Timestamp) {
        // only add proposals from within the current round
        if state::round_id(timestamp, self.current_round_len) == self.current_round_id {
            trace!(
                %self.current_round_id,
                timestamp = timestamp.millis(),
                "adding a proposal"
            );
            self.proposals.push(proposal_h);
        } else {
            trace!(
                %self.current_round_id,
                timestamp = timestamp.millis(),
                %self.current_round_len,
                "trying to add proposal for a different round!"
            );
        }
    }

    /// If the current timestamp indicates that the round has ended, checks the known proposals for
    /// a level-1 summit.
    /// If there is a summit, the round is considered successful. Otherwise, it is considered
    /// failed.
    /// Next, a number of last rounds are being checked for success and if not enough of them are
    /// successful, we return a higher round length for the future.
    /// If the length shouldn't grow, and the round ID is divisible by a certain number, a lower
    /// round length is returned.
    pub fn calculate_new_length(&mut self, state: &State<C>) -> TimeDiff {
        let now = Timestamp::now();
        // if the round hasn't finished, just return whatever we have now
        if state::round_id(now, self.current_round_len) <= self.current_round_id {
            return self.new_length();
        }

        trace!(%self.current_round_id, "calculating length");
        let current_round_index = round_index(self.current_round_id, self.current_round_len);
        let new_round_index = round_index(now, self.current_round_len);

        if mem::take(&mut self.proposals)
            .into_iter()
            .any(|proposal| self.check_proposals_success(state, &proposal))
        {
            trace!("round succeeded");
            self.rounds.push_front(true);
        } else {
            trace!("round failed");
            self.rounds.push_front(false);
        }

        // if we're just switching rounds and more than a single round has passed, all the
        // rounds since the last registered round have failed
        let failed_round_count = new_round_index
            .saturating_sub(current_round_index)
            .saturating_sub(1);
        for _ in 0..failed_round_count {
            trace!("round failed");
            self.rounds.push_front(false);
        }

        self.current_round_id =
            Timestamp::zero() + self.current_round_len.saturating_mul(new_round_index);

        self.clean_old_rounds();

        trace!(
            %self.current_round_len,
            "{} failures among the last {} rounds.",
            self.count_failures(),
            self.rounds.len()
        );

        let new_len = self.new_length();

        trace!(%new_len, "new length calculated");

        if new_len != self.current_round_len {
            self.change_length(new_len, now);
        }

        new_len
    }

    /// Returns an instance of `Self` for the new era: resetting the counters where appropriate.
    pub fn next_era(&self, timestamp: Timestamp) -> Self {
        Self {
            rounds: self.rounds.clone(),
            current_round_id: state::round_id(timestamp, self.current_round_len),
            proposals: Default::default(),
            min_round_len: self.min_round_len,
            max_round_len: self.max_round_len,
            current_round_len: self.current_round_len,
            config: self.config,
        }
    }

    fn clean_old_rounds(&mut self) {
        while self.rounds.len() as u64 > self.config.num_rounds_to_consider {
            self.rounds.pop_back();
        }
    }

    fn count_failures(&self) -> usize {
        self.rounds.iter().filter(|&success| !success).count()
    }

    /// Returns the round length to be used in the next round, based on the previously used round
    /// length and the current counts of successes and failures.
    pub(super) fn new_length(&self) -> TimeDiff {
        let current_round_index = round_index(self.current_round_id, self.current_round_len);
        let num_failures = self.count_failures() as u64;
        #[allow(clippy::integer_arithmetic)] // The acceleration_parameter is not zero.
        if num_failures > self.config.max_failed_rounds()
            && self.current_round_len * 2 <= self.max_round_len
        {
            self.current_round_len * 2
        } else if current_round_index % self.config.acceleration_parameter == 0
            && self.current_round_len > self.min_round_len
            // we will only accelerate if we collected data about enough rounds
            && self.rounds.len() as u64 == self.config.num_rounds_to_consider
            && num_failures < self.config.max_failures_for_acceleration()
        {
            self.current_round_len / 2
        } else {
            self.current_round_len
        }
    }
}

/// Returns the round index `i`, if `r_id` is the ID of the `i`-th round after the epoch.
#[allow(clippy::integer_arithmetic)] // Checking for division by 0.
fn round_index(r_id: Timestamp, round_len: TimeDiff) -> u64 {
    if round_len.millis() == 0 {
        error!("called round_index with round_len 0.");
        return r_id.millis();
    }
    r_id.millis() / round_len.millis()
}

#[cfg(test)]
mod tests {
    use config::{Config, ACCELERATION_PARAMETER, MAX_FAILED_ROUNDS, NUM_ROUNDS_TO_CONSIDER};

    use casper_types::{TimeDiff, Timestamp};

    use crate::components::consensus::{
        cl_context::ClContext,
        protocols::highway::round_success_meter::{config, round_index},
    };

    const TEST_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 13);
    const TEST_MIN_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 8);
    const TEST_MAX_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 19);

    #[test]
    fn new_length_steady() {
        let round_success_meter: super::RoundSuccessMeter<ClContext> =
            super::RoundSuccessMeter::new(
                TEST_ROUND_LEN,
                TEST_MIN_ROUND_LEN,
                TEST_MAX_ROUND_LEN,
                Timestamp::now(),
                Config::default(),
            );
        assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN);
    }

    #[test]
    fn new_length_slow_down() {
        let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
            super::RoundSuccessMeter::new(
                TEST_ROUND_LEN,
                TEST_MIN_ROUND_LEN,
                TEST_MAX_ROUND_LEN,
                Timestamp::now(),
                Config::default(),
            );
        // If there have been more rounds of failure than MAX_FAILED_ROUNDS, slow down
        round_success_meter.rounds = vec![false; MAX_FAILED_ROUNDS + 1].into();
        assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN * 2);
    }

    #[test]
    fn new_length_can_not_slow_down_because_max_round_len() {
        // If the round length is the same as the maximum round length, can't go up
        let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
            super::RoundSuccessMeter::new(
                TEST_MAX_ROUND_LEN,
                TEST_MIN_ROUND_LEN,
                TEST_MAX_ROUND_LEN,
                Timestamp::now(),
                Config::default(),
            );
        // If there have been more rounds of failure than MAX_FAILED_ROUNDS, slow down -- but can't
        // slow down because of ceiling
        round_success_meter.rounds = vec![false; MAX_FAILED_ROUNDS + 1].into();
        assert_eq!(round_success_meter.new_length(), TEST_MAX_ROUND_LEN);
    }

    #[test]
    fn new_length_speed_up() {
        // If there's been enough successful rounds and it's an acceleration round, speed up
        let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
            super::RoundSuccessMeter::new(
                TEST_ROUND_LEN,
                TEST_MIN_ROUND_LEN,
                TEST_MAX_ROUND_LEN,
                Timestamp::now(),
                Config::default(),
            );
        round_success_meter.rounds = vec![true; NUM_ROUNDS_TO_CONSIDER].into();
        // Increase our round index until we are at an acceleration round
        loop {
            let current_round_index = round_index(
                round_success_meter.current_round_id,
                round_success_meter.current_round_len,
            );
            if current_round_index % ACCELERATION_PARAMETER == 0 {
                break;
            };
            round_success_meter.current_round_id += TimeDiff::from_millis(1);
        }
        assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN / 2);
    }

    #[test]
    fn new_length_can_not_speed_up_because_min_round_len() {
        // If there's been enough successful rounds and it's an acceleration round, but we are
        // already at the smallest round length possible, stay at the current round length
        let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
            super::RoundSuccessMeter::new(
                TEST_MIN_ROUND_LEN,
                TEST_MIN_ROUND_LEN,
                TEST_MAX_ROUND_LEN,
                Timestamp::now(),
                Config::default(),
            );
        round_success_meter.rounds = vec![true; NUM_ROUNDS_TO_CONSIDER].into();
        // Increase our round index until we are at an acceleration round
        loop {
            let current_round_index = round_index(
                round_success_meter.current_round_id,
                round_success_meter.current_round_len,
            );
            if current_round_index % ACCELERATION_PARAMETER == 0 {
                break;
            };
            round_success_meter.current_round_id += TimeDiff::from_millis(1);
        }
        assert_eq!(round_success_meter.new_length(), TEST_MIN_ROUND_LEN);
    }
}
