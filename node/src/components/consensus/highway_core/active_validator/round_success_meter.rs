use std::collections::VecDeque;

use crate::types::Timestamp;

const NUM_ROUNDS_TO_CONSIDER: usize = 40;
const MAX_FAILED_ROUNDS: usize = 10;
const ACCELERATION_PARAMETER: usize = 1000;

pub(super) struct RoundSuccessMeter {
    // store whether a particular round was successful
    // index 0 is the last handled round, 1 is the second-to-last etc.
    rounds: VecDeque<bool>,
    round_exp: u8,
    round_length: u64,
    last_exponent_change_index: usize,
    latest_handled_round_index: Option<usize>,
}

impl RoundSuccessMeter {
    pub fn new(round_exp: u8, timestamp: Timestamp) -> Self {
        let round_length = 1u64 << round_exp;
        Self {
            rounds: VecDeque::with_capacity(NUM_ROUNDS_TO_CONSIDER),
            round_exp,
            round_length,
            last_exponent_change_index: (timestamp.millis() / round_length) as usize,
            latest_handled_round_index: None,
        }
    }

    pub fn change_exponent(&mut self, new_exp: u8, timestamp: Timestamp) {
        self.rounds = VecDeque::with_capacity(NUM_ROUNDS_TO_CONSIDER);
        self.round_exp = new_exp;
        self.round_length = 1u64 << new_exp;
        self.last_exponent_change_index = (timestamp.millis() / self.round_length) as usize;
        self.latest_handled_round_index = None;
    }

    /// Registers the success or failure of finalizing a block within a round, and returns the new
    /// round exponent based on whether we are keeping up with the blocks or not.
    pub fn handle_finalized_block(
        &mut self,
        block_timestamp: Timestamp,
        time_finalized: Timestamp,
    ) -> u8 {
        // in case the block timestamp isn't at the beginning of the round
        let block_round_start_index = (block_timestamp.millis() / self.round_length) as usize;
        let finalization_round_index = (time_finalized.millis() / self.round_length) as usize;
        let round_successful = finalization_round_index == block_round_start_index;
        // if we finalized a block at round N, and the last round we handled is N-K, we mark all
        // rounds between rounds N-K+1 and N-1 as failed
        let num_rounds_to_fail = self.latest_handled_round_index.map_or(
            block_round_start_index - self.last_exponent_change_index,
            |latest| block_round_start_index.saturating_sub(latest + 1),
        );
        for _ in 0..num_rounds_to_fail {
            self.rounds.push_front(false);
        }
        // mark round N as successful
        // this will be None if latest_handled_round_index was None, or block round index was
        // greater than the latest round index
        let round_index_to_mark = self
            .latest_handled_round_index
            .and_then(|latest| latest.checked_sub(block_round_start_index));
        match round_index_to_mark {
            None => {
                self.rounds.push_front(round_successful);
                self.latest_handled_round_index = Some(block_round_start_index);
            }
            Some(index) if round_successful => {
                if let Some(success_ref) = self.rounds.get_mut(index) {
                    *success_ref = true;
                }
            }
            _ => (),
        }
        self.clean_old_rounds();
        self.new_exponent(block_round_start_index)
    }

    fn clean_old_rounds(&mut self) {
        while self.rounds.len() > NUM_ROUNDS_TO_CONSIDER {
            self.rounds.pop_back();
        }
    }

    fn count_failures(&self) -> usize {
        self.rounds.iter().filter(|&success| !success).count()
    }

    fn new_exponent(&self, block_round_start_index: usize) -> u8 {
        if block_round_start_index % 2 == 0 && self.count_failures() > MAX_FAILED_ROUNDS {
            self.round_exp + 1
        } else if block_round_start_index % ACCELERATION_PARAMETER == 0 {
            self.round_exp - 1
        } else {
            self.round_exp
        }
    }
}
