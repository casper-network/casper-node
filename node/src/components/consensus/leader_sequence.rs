use datasize::DataSize;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;
use tracing::error;

use crate::components::consensus::highway_core::{
    state::weight::Weight,
    validators::{ValidatorIndex, ValidatorMap},
};

/// A pseudorandom sequence of validator indices, distributed by weight.
#[derive(Debug, Clone, DataSize, Serialize)]
pub(crate) struct LeaderSequence {
    /// Cumulative validator weights: Entry `i` contains the sum of the weights of validators `0`
    /// through `i`.
    cumulative_w: ValidatorMap<Weight>,
    /// Cumulative validator weights, but with the weight of banned validators set to `0`.
    cumulative_w_leaders: ValidatorMap<Weight>,
    /// This is `false` for validators who have been excluded from the sequence.
    leaders: ValidatorMap<bool>,
    /// The PRNG seed.
    seed: u64,
}

impl LeaderSequence {
    pub(crate) fn new(
        seed: u64,
        weights: &ValidatorMap<Weight>,
        leaders: ValidatorMap<bool>,
    ) -> LeaderSequence {
        let sums = |mut sums: Vec<Weight>, w: Weight| {
            let sum = sums.last().copied().unwrap_or(Weight(0));
            sums.push(sum.checked_add(w).expect("total weight must be < 2^64"));
            sums
        };
        let cumulative_w = ValidatorMap::from(weights.iter().copied().fold(vec![], sums));
        assert!(
            *cumulative_w.as_ref().last().unwrap() > Weight(0),
            "total weight must not be zero"
        );
        let cumulative_w_leaders = weights
            .enumerate()
            .map(|(idx, weight)| leaders[idx].then(|| *weight).unwrap_or(Weight(0)))
            .fold(vec![], sums)
            .into();
        LeaderSequence {
            cumulative_w,
            cumulative_w_leaders,
            leaders,
            seed,
        }
    }

    /// Returns the leader in the specified slot.
    ///
    /// First the assignment is computed ignoring the `leaders` flags. Only if the selected
    /// leader's entry is `false`, the computation is repeated, this time with the flagged
    /// validators excluded. This ensures that once the validator set has been decided, correct
    /// validators' slots never get reassigned to someone else, even if after the fact someone is
    /// excluded as a leader.
    pub(crate) fn leader(&self, slot: u64) -> ValidatorIndex {
        // The binary search cannot return None; if it does, it's a programming error. In that case,
        // we want the tests to panic but production to pick a default.
        let panic_or_0 = || {
            if cfg!(test) {
                panic!("random number out of range");
            } else {
                error!("random number out of range");
                ValidatorIndex(0)
            }
        };
        let seed = self.seed.wrapping_add(slot);
        // We select a random one out of the `total_weight` weight units, starting numbering at 1.
        let r = Weight(leader_prng(self.total_weight().0, seed));
        // The weight units are subdivided into intervals that belong to some validator.
        // `cumulative_w[i]` denotes the last weight unit that belongs to validator `i`.
        // `binary_search` returns the first `i` with `cumulative_w[i] >= r`, i.e. the validator
        // who owns the randomly selected weight unit.
        let leader_index = self
            .cumulative_w
            .binary_search(&r)
            .unwrap_or_else(panic_or_0);
        if self.leaders[leader_index] {
            return leader_index;
        }
        // If the selected leader is excluded, we reassign the slot to someone else. This time we
        // consider only the non-banned validators.
        let total_w_leaders = *self.cumulative_w_leaders.as_ref().last().unwrap();
        let r = Weight(leader_prng(total_w_leaders.0, seed.wrapping_add(1)));
        self.cumulative_w_leaders
            .binary_search(&r)
            .unwrap_or_else(panic_or_0)
    }

    /// Returns the sum of all validators' voting weights.
    pub(crate) fn total_weight(&self) -> Weight {
        *self
            .cumulative_w
            .as_ref()
            .last()
            .expect("weight list cannot be empty")
    }
}

/// Returns a pseudorandom `u64` between `1` and `upper` (inclusive).
fn leader_prng(upper: u64, seed: u64) -> u64 {
    ChaCha8Rng::seed_from_u64(seed)
        .gen_range(0..upper)
        .saturating_add(1)
}

/// Returns a seed that with the given weights results in the desired leader sequence.
#[cfg(test)]
pub(crate) fn find_seed(
    seq: &[ValidatorIndex],
    weights: &ValidatorMap<Weight>,
    leaders: &ValidatorMap<bool>,
) -> u64 {
    for seed in 0..1000 {
        let ls = LeaderSequence::new(seed, weights, leaders.clone());
        if seq
            .iter()
            .enumerate()
            .all(|(slot, &v_idx)| ls.leader(slot as u64) == v_idx)
        {
            return seed;
        }
    }
    panic!("No suitable seed for leader sequence found");
}

#[test]
fn test_leader_prng() {
    use rand::RngCore;

    let mut rng = crate::new_rng();

    // Repeat a few times to make it likely that the inner loop runs more than once.
    for _ in 0..10 {
        let upper = rng.gen_range(1..u64::MAX);
        let seed = rng.next_u64();

        // This tests that the rand crate's gen_range implementation, which is used in
        // leader_prng, doesn't change, and uses this algorithm:
        // https://github.com/rust-random/rand/blob/73befa480c58dd0461da5f4469d5e04c564d4de3/src/distributions/uniform.rs#L515
        let mut prng = ChaCha8Rng::seed_from_u64(seed);
        let zone = upper << upper.leading_zeros(); // A multiple of upper that fits into a u64.
        let expected = loop {
            // Multiply a random u64 by upper. This is between 0 and u64::MAX * upper.
            let prod = (prng.next_u64() as u128) * (upper as u128);
            // So prod >> 64 is between 0 and upper - 1. Each interval from (N << 64) to
            // (N << 64) + zone contains the same number of such values.
            // If the value is in such an interval, return N + 1; otherwise retry.
            if (prod as u64) < zone {
                break (prod >> 64) as u64 + 1;
            }
        };

        assert_eq!(expected, leader_prng(upper, seed));
    }
}

#[test]
fn test_leader_prng_values() {
    // Test a few concrete values, to detect if the ChaCha8Rng impl changes.
    assert_eq!(12578764544318200737, leader_prng(u64::MAX, 42));
    assert_eq!(12358540700710939054, leader_prng(u64::MAX, 1337));
    assert_eq!(4134160578770126600, leader_prng(u64::MAX, 0x1020304050607));
}
