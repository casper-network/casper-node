use std::convert::identity;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{
    Context, Observation, Panorama, Timestamp, ValidatorIndex, ValidatorMap, Weight, BLOCK_REWARD,
};

/// Parameters for a Highway `State`.
#[derive(Debug)]
pub(crate) struct Params {
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// Cumulative validator weights: Entry `i` contains the sum of the weights of validators `0`
    /// through `i`.
    cumulative_w: ValidatorMap<Weight>,
    /// The random seed.
    seed: u64,
    /// The reduced block reward that is paid out even if the heaviest summit does not exceed half
    /// the total weight. This is a fraction of `BLOCK_REWARD`.
    reduced_block_reward: u64,
    /// The minimum round exponent. `1 << min_round_exp` milliseconds is the minimum round length.
    min_round_exp: u8,
}

impl Params {
    pub(crate) fn new(
        weights: ValidatorMap<Weight>,
        seed: u64,
        (ff_num, ff_denom): (u16, u16),
        min_round_exp: u8,
    ) -> Params {
        assert!(
            ff_num <= ff_denom,
            "forgiveness factor must be at most 100%"
        );
        let mut sum = Weight(0);
        let add = |w: &Weight| {
            sum += *w;
            sum
        };
        let cumulative_w = weights.iter().map(add).collect();
        Params {
            weights,
            cumulative_w,
            seed,
            reduced_block_reward: BLOCK_REWARD * u64::from(ff_num) / u64::from(ff_denom),
            min_round_exp,
        }
    }

    /// Returns the number of validators.
    pub(crate) fn validator_count(&self) -> usize {
        self.weights.len()
    }

    /// Returns the `idx`th validator's voting weight.
    pub(crate) fn weight(&self, idx: ValidatorIndex) -> Weight {
        self.weights[idx]
    }

    /// Returns the map of validator weights.
    pub(crate) fn weights(&self) -> &ValidatorMap<Weight> {
        &self.weights
    }

    /// Returns the reduced block reward that is paid out even if the heaviest summit does not
    /// exceed half the total weight. This is a fraction of `BLOCK_REWARD`.
    pub(crate) fn reduced_block_reward(&self) -> u64 {
        self.reduced_block_reward
    }

    /// Returns the total weight of all validators marked faulty in this panorama.
    pub(crate) fn faulty_weight_in<C: Context>(&self, panorama: &Panorama<C>) -> Weight {
        panorama
            .iter()
            .zip(&self.weights)
            .filter(|(obs, _)| **obs == Observation::Faulty)
            .map(|(_, w)| *w)
            .sum()
    }

    /// Returns the sum of all validators' voting weights.
    pub(crate) fn total_weight(&self) -> Weight {
        *self.cumulative_w.as_ref().last().unwrap()
    }

    /// Returns the minimum round exponent. `1 << self.min_round_exp()` milliseconds is the minimum
    /// round length.
    pub(crate) fn min_round_exp(&self) -> u8 {
        self.min_round_exp
    }

    /// Returns the leader in the specified time slot.
    pub(crate) fn leader(&self, timestamp: Timestamp) -> ValidatorIndex {
        let mut rng = ChaCha8Rng::seed_from_u64(self.seed.wrapping_add(timestamp.millis()));
        // TODO: `rand` doesn't seem to document how it generates this. Needs to be portable.
        // We select a random one out of the `total_weight` weight units, starting numbering at 1.
        let r = Weight(rng.gen_range(1, self.total_weight().0 + 1));
        // The weight units are subdivided into intervals that belong to some validator.
        // `cumulative_w[i]` denotes the last weight unit that belongs to validator `i`.
        // `binary_search` returns the first `i` with `cumulative_w[i] >= r`, i.e. the validator
        // who owns the randomly selected weight unit.
        self.cumulative_w.binary_search(&r).unwrap_or_else(identity)
    }
}
