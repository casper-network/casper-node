use super::Timestamp;

/// Protocol parameters for Highway.
#[derive(Debug)]
pub(crate) struct Params {
    seed: u64,
    block_reward: u64,
    reduced_block_reward: u64,
    reward_delay: u64,
    min_round_exp: u8,
    end_height: u64,
    end_timestamp: Timestamp,
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
    /// * `reward_delay`: The delay after which rewards are calculated. Rewards for a round in which
    ///   a block B was proposed are paid out in the first block whose timestamp greater than
    ///   `reward_delay * t` after B's timestamp, where `t` is the round length of B itself.
    /// * `min_round_exp`: The minimum round exponent. `1 << min_round_exp` milliseconds is the
    ///   minimum round length.
    /// * `end_height`, `end_timestamp`: The last block will be the first one that has at least the
    ///   specified height _and_ is no earlier than the specified timestamp. No children of this
    ///   block can be proposed.
    pub(crate) fn new(
        seed: u64,
        block_reward: u64,
        reduced_block_reward: u64,
        reward_delay: u64,
        min_round_exp: u8,
        end_height: u64,
        end_timestamp: Timestamp,
    ) -> Params {
        assert!(
            reduced_block_reward <= block_reward,
            "reduced block reward must not be greater than the reward for a finalized block"
        );
        Params {
            seed,
            block_reward,
            reduced_block_reward,
            reward_delay,
            min_round_exp,
            end_height,
            end_timestamp,
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

    /// Returns the delay after which rewards for a block are paid out, in multiples of the round
    /// length of that block.
    pub(crate) fn reward_delay(&self) -> u64 {
        self.reward_delay
    }

    /// Returns the minimum round exponent. `1 << self.min_round_exp()` milliseconds is the minimum
    /// round length.
    pub(crate) fn min_round_exp(&self) -> u8 {
        self.min_round_exp
    }

    /// Returns the minimum height of the last block.
    pub(crate) fn end_height(&self) -> u64 {
        self.end_height
    }

    /// Returns the minimum timestamp of the last block.
    pub(crate) fn end_timestamp(&self) -> Timestamp {
        self.end_timestamp
    }
}
