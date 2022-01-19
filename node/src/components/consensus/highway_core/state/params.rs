use datasize::DataSize;
use serde::Serialize;

use super::{round_len, TimeDiff, Timestamp};

/// Protocol parameters for Highway.
#[derive(Debug, DataSize, Clone, Serialize)]
pub(crate) struct Params {
    seed: u64,
    block_reward: u64,
    reduced_block_reward: u64,
    min_round_exp: u8,
    max_round_exp: u8,
    init_round_exp: u8,
    end_height: u64,
    start_timestamp: Timestamp,
    end_timestamp: Timestamp,
    endorsement_evidence_limit: u64,
}

impl Params {
    /// Creates a new set of Highway protocol parameters.
    ///
    /// Arguments:
    ///
    /// * `seed`: The random seed.
    /// * `block_reward`: The total reward that is paid out for a finalized block. Validator rewards
    ///   for finalization must add up to this number or less. This should be large enough to allow
    ///   very precise fractions of a block reward while still leaving space for millions of full
    ///   rewards in a `u64`.
    /// * `reduced_block_reward`: The reduced block reward that is paid out even if the heaviest
    ///   summit does not exceed half the total weight.
    /// * `min_round_exp`: The minimum round exponent. `1 << min_round_exp` milliseconds is the
    ///   minimum round length.
    /// * `max_round_exp`: The maximum round exponent. `1 << max_round_exp` milliseconds is the
    ///   maximum round length.
    /// * `end_height`, `end_timestamp`: The last block will be the first one that has at least the
    ///   specified height _and_ is no earlier than the specified timestamp. No children of this
    ///   block can be proposed.
    #[allow(clippy::too_many_arguments)] // FIXME
    pub(crate) fn new(
        seed: u64,
        block_reward: u64,
        reduced_block_reward: u64,
        min_round_exp: u8,
        max_round_exp: u8,
        init_round_exp: u8,
        end_height: u64,
        start_timestamp: Timestamp,
        end_timestamp: Timestamp,
        endorsement_evidence_limit: u64,
    ) -> Params {
        assert!(
            reduced_block_reward <= block_reward,
            "reduced block reward must not be greater than the reward for a finalized block"
        );
        Params {
            seed,
            block_reward,
            reduced_block_reward,
            min_round_exp,
            max_round_exp,
            init_round_exp,
            end_height,
            start_timestamp,
            end_timestamp,
            endorsement_evidence_limit,
        }
    }

    /// Returns the random seed.
    pub(crate) fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns the total reward for a finalized block.
    pub(crate) fn block_reward(&self) -> u64 {
        self.block_reward
    }

    /// Returns the reduced block reward that is paid out even if the heaviest summit does not
    /// exceed half the total weight. This is at most `block_reward`.
    pub(crate) fn reduced_block_reward(&self) -> u64 {
        self.reduced_block_reward
    }

    /// Returns the minimum round exponent. `1 << self.min_round_exp()` milliseconds is the minimum
    /// round length.
    pub(crate) fn min_round_exp(&self) -> u8 {
        self.min_round_exp
    }

    /// Returns the maximum round exponent. `1 << self.max_round_exp()` milliseconds is the maximum
    /// round length.
    pub(crate) fn max_round_exp(&self) -> u8 {
        self.max_round_exp
    }

    /// Returns the minimum round length, corresponding to the minimum round exponent.
    pub(crate) fn min_round_length(&self) -> TimeDiff {
        round_len(self.min_round_exp)
    }

    /// Returns the maximum round length, corresponding to the maximum round exponent.
    pub(crate) fn max_round_length(&self) -> TimeDiff {
        round_len(self.max_round_exp)
    }

    /// Returns the initial round exponent.
    pub(crate) fn init_round_exp(&self) -> u8 {
        self.init_round_exp
    }

    /// Returns the minimum height of the last block.
    pub(crate) fn end_height(&self) -> u64 {
        self.end_height
    }

    /// Returns the start timestamp of the era.
    pub(crate) fn start_timestamp(&self) -> Timestamp {
        self.start_timestamp
    }

    /// Returns the minimum timestamp of the last block.
    pub(crate) fn end_timestamp(&self) -> Timestamp {
        self.end_timestamp
    }

    /// Returns the maximum number of additional units included in evidence for conflicting
    /// endorsements. If you endorse two conflicting forks at sequence numbers that differ by more
    /// than this, you get away with it and are not marked faulty.
    pub(crate) fn endorsement_evidence_limit(&self) -> u64 {
        self.endorsement_evidence_limit
    }
}

#[cfg(test)]
impl Params {
    pub(crate) fn with_endorsement_evidence_limit(mut self, new_limit: u64) -> Params {
        self.endorsement_evidence_limit = new_limit;
        self
    }

    pub(crate) fn with_max_round_exp(mut self, new_max_round_exp: u8) -> Params {
        self.max_round_exp = new_max_round_exp;
        self
    }

    pub(crate) fn with_end_height(mut self, new_end_height: u64) -> Params {
        self.end_height = new_end_height;
        self
    }
}
