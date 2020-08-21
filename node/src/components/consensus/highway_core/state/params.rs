use super::{Timestamp, BLOCK_REWARD};

/// Parameters for a Highway `State`.
#[derive(Debug)]
pub(crate) struct Params {
    /// The random seed.
    seed: u64,
    /// The reduced block reward that is paid out even if the heaviest summit does not exceed half
    /// the total weight. This is a fraction of `BLOCK_REWARD`.
    reduced_block_reward: u64,
    /// The minimum round exponent. `1 << min_round_exp` milliseconds is the minimum round length.
    min_round_exp: u8,
    /// The minimum height of the last block.
    end_height: u64,
    /// The minimum timestamp of the last block.
    end_timestamp: Timestamp,
}

impl Params {
    pub(crate) fn new(
        seed: u64,
        (ff_num, ff_denom): (u16, u16),
        min_round_exp: u8,
        end_height: u64,
        end_timestamp: Timestamp,
    ) -> Params {
        assert!(
            ff_num <= ff_denom,
            "forgiveness factor must be at most 100%"
        );
        Params {
            seed,
            reduced_block_reward: BLOCK_REWARD * u64::from(ff_num) / u64::from(ff_denom),
            min_round_exp,
            end_height,
            end_timestamp,
        }
    }

    /// Returns the random seed.
    pub(crate) fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns the reduced block reward that is paid out even if the heaviest summit does not
    /// exceed half the total weight. This is a fraction of `BLOCK_REWARD`.
    pub(crate) fn reduced_block_reward(&self) -> u64 {
        self.reduced_block_reward
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
