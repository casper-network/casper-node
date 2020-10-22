use std::{cmp::max, collections::VecDeque};

use datasize::DataSize;

use crate::{
    components::consensus::{
        highway_core::{finality_detector::FinalityDetector, State, Weight},
        traits::Context,
    },
    types::Timestamp,
};

const NUM_ROUNDS_TO_CONSIDER: usize = 40;
const MAX_FAILED_ROUNDS: usize = 10;
const ACCELERATION_PARAMETER: u64 = 1000;
const THRESHOLD: u64 = 1;

#[derive(DataSize, Debug)]
pub(crate) struct RoundSuccessMeter<C>
where
    C: Context,
{
    // store whether a particular round was successful
    // index 0 is the last handled round, 1 is the second-to-last etc.
    rounds: VecDeque<bool>,
    current_round_id: u64,
    proposals: Vec<C::Hash>,
    current_round_exp: u8,
    current_round_length: u64,
}

impl<C: Context> RoundSuccessMeter<C> {
    pub fn new(round_exp: u8, timestamp: Timestamp) -> Self {
        let current_round_length = 1u64 << round_exp;
        let current_round_index = timestamp.millis() / current_round_length;
        let current_round_id = current_round_index * current_round_length;
        Self {
            rounds: VecDeque::with_capacity(NUM_ROUNDS_TO_CONSIDER),
            current_round_id,
            proposals: Vec::new(),
            current_round_exp: round_exp,
            current_round_length,
        }
    }

    fn change_exponent(&mut self, new_exp: u8, timestamp: Timestamp) {
        self.rounds = VecDeque::with_capacity(NUM_ROUNDS_TO_CONSIDER);
        self.current_round_exp = new_exp;
        self.current_round_length = 1u64 << new_exp;
        let round_index = timestamp.millis() / self.current_round_length;
        self.current_round_id = round_index * self.current_round_length;
        self.proposals = Vec::new();
    }

    fn check_proposals_success(&self, state: &State<C>, proposal_h: &C::Hash) -> bool {
        let total_w = state.total_weight();

        let finality_detector =
            FinalityDetector::<C>::new(max(total_w * THRESHOLD / 100, Weight(1)));

        // check for the existence of a level-1 summit
        finality_detector.find_summit(1, proposal_h, state) == 1
    }

    /// Registers a proposal within this round - if it's finalized within the round, the round will
    /// be successful.
    pub fn new_proposal(&mut self, proposal_h: C::Hash, timestamp: Timestamp) {
        // only add proposals from within the current round
        if timestamp.millis() >= self.current_round_id
            && timestamp.millis() - self.current_round_id < self.current_round_length
        {
            self.proposals.push(proposal_h);
        }
    }

    /// If the current timestamp indicates that the round has ended, checks the known proposals for
    /// a level-1 summit.
    /// If there is a summit, the round is considered successful. Otherwise, it is considered
    /// failed.
    /// Next, a number of last rounds are being checked for success and if not enough of them are
    /// successful, we return a higher round exponent for the future.
    /// If the exponent shouldn't grow, and the round ID is divisible by a certain number, a lower
    /// round exponent is returned.
    pub fn calculate_new_exponent(&mut self, state: &State<C>) -> u8 {
        let now = Timestamp::now();
        // if the round hasn't finished, just return whatever we have now
        if now.millis().saturating_sub(self.current_round_id) < self.current_round_length {
            return self.new_exponent();
        }

        let current_round_index = self.current_round_id / self.current_round_length;
        let new_round_index = now.millis() / self.current_round_length;

        if self
            .proposals
            .iter()
            .any(|proposal| self.check_proposals_success(state, proposal))
        {
            self.rounds.push_front(true);
        } else {
            self.rounds.push_front(false);
        }

        // if we're just switching rounds and more than a single round has passed, all the
        // rounds since the last registered round have failed
        for _ in 0..(new_round_index - current_round_index - 1) {
            self.rounds.push_front(false);
        }

        self.current_round_id = new_round_index * self.current_round_length;

        self.clean_old_rounds();

        let new_exp = self.new_exponent();

        if new_exp != self.current_round_exp {
            self.change_exponent(new_exp, now);
        }

        new_exp
    }

    fn clean_old_rounds(&mut self) {
        while self.rounds.len() > NUM_ROUNDS_TO_CONSIDER {
            self.rounds.pop_back();
        }
    }

    fn count_failures(&self) -> usize {
        self.rounds.iter().filter(|&success| !success).count()
    }

    fn new_exponent(&self) -> u8 {
        let current_round_index = self.current_round_id / self.current_round_length;
        if self.count_failures() > MAX_FAILED_ROUNDS {
            self.current_round_exp + 1
        } else if current_round_index % ACCELERATION_PARAMETER == 0 {
            self.current_round_exp - 1
        } else {
            self.current_round_exp
        }
    }
}
