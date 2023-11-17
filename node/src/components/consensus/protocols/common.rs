//! Utilities common to different consensus algorithms.

use itertools::Itertools;
use num_rational::Ratio;
use std::collections::{BTreeMap, HashSet};

use num_traits::AsPrimitive;

use crate::components::consensus::{
    traits::Context,
    utils::{ValidatorMap, Validators, Weight},
};
use casper_types::U512;

/// Computes the validator set given the stakes and the faulty and inactive
/// reports from the previous eras.
pub fn validators<C: Context>(
    faulty: &HashSet<C::ValidatorId>,
    inactive: &HashSet<C::ValidatorId>,
    validator_stakes: BTreeMap<C::ValidatorId, U512>,
) -> Validators<C::ValidatorId> {
    let sum_stakes = safe_sum(validator_stakes.values().copied()).expect("should not overflow");
    // We use u64 weights. Scale down by floor(sum / u64::MAX) + 1.
    // This guarantees that the resulting sum is less than u64::MAX.
    let scaling_factor: U512 = sum_stakes / U512::from(u64::MAX) + 1;

    // TODO sort validators by descending weight
    let mut validators: Validators<C::ValidatorId> = validator_stakes
        .into_iter()
        .map(|(key, stake)| (key, AsPrimitive::<u64>::as_(stake / scaling_factor)))
        .collect();

    for vid in faulty {
        validators.ban(vid);
    }

    for vid in inactive {
        validators.set_cannot_propose(vid);
    }

    assert!(
        validators.ensure_nonzero_proposing_stake(),
        "cannot start era with total weight 0"
    );

    validators
}

/// Compute the validator weight map from the set of validators.
pub(crate) fn validator_weights<C: Context>(
    validators: &Validators<C::ValidatorId>,
) -> ValidatorMap<Weight> {
    ValidatorMap::from(validators.iter().map(|v| v.weight()).collect_vec())
}

/// Computes the fault tolerance threshold for the protocol instance
pub(crate) fn ftt<C: Context>(
    finality_threshold_fraction: Ratio<u64>,
    validators: &Validators<C::ValidatorId>,
) -> Weight {
    let total_weight = u128::from(validators.total_weight());
    assert!(
        finality_threshold_fraction < 1.into(),
        "finality threshold must be less than 100%"
    );
    #[allow(clippy::integer_arithmetic)] // FTT is less than 1, so this can't overflow
    let ftt = total_weight * *finality_threshold_fraction.numer() as u128
        / *finality_threshold_fraction.denom() as u128;
    (ftt as u64).into()
}

/// A U512 sum implementation that check for overflow.
fn safe_sum<I>(mut iterator: I) -> Option<U512>
where
    I: Iterator<Item = U512>,
{
    iterator.try_fold(U512::zero(), |acc, n| acc.checked_add(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::consensus::ClContext;
    use casper_types::{testing::TestRng, PublicKey};
    use rand::Rng;

    #[test]
    #[should_panic]
    fn ftt_panics_during_overflow() {
        let rng = &mut TestRng::new();
        let mut validator_stakes = BTreeMap::new();
        validator_stakes.insert(PublicKey::random(rng), U512::MAX);
        validator_stakes.insert(PublicKey::random(rng), U512::from(1_u32));

        validators::<ClContext>(&Default::default(), &Default::default(), validator_stakes);
    }

    #[test]
    fn total_weights_less_than_u64_max() {
        let mut rng = TestRng::new();

        let (test_stake_1, test_stake_2) = (rng.gen(), rng.gen());

        let mut test_stakes = |a: u64, b: u64| -> BTreeMap<PublicKey, U512> {
            let mut result = BTreeMap::new();
            result.insert(
                PublicKey::random(&mut rng),
                U512::from(a) * U512::from(u128::MAX),
            );
            result.insert(
                PublicKey::random(&mut rng),
                U512::from(b) * U512::from(u128::MAX),
            );
            result
        };

        // First, we test with random values.
        let stakes = test_stakes(test_stake_1, test_stake_2);
        let weights = validators::<ClContext>(&Default::default(), &Default::default(), stakes);
        assert!(weights.total_weight().0 < u64::MAX);

        // Then, we test with values that were known to cause issues before.
        let stakes = test_stakes(514, 771);
        let weights = validators::<ClContext>(&Default::default(), &Default::default(), stakes);
        assert!(weights.total_weight().0 < u64::MAX);

        let stakes = test_stakes(668, 614);
        let weights = validators::<ClContext>(&Default::default(), &Default::default(), stakes);
        assert!(weights.total_weight().0 < u64::MAX);
    }
}
