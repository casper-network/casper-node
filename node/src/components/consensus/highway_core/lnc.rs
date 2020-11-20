use std::collections::{BTreeSet, HashSet};

use crate::components::consensus::traits::Context;

use super::{
    state::{Observation, Panorama},
    validators::ValidatorIndex,
    State,
};

pub(crate) enum LncForks<C: Context> {
    None,
    Single(C::Hash),
    Multiple,
}

impl<C: Context> LncForks<C> {
    pub(crate) fn is_none(&self) -> bool {
        match self {
            LncForks::None => true,
            LncForks::Single(_) | LncForks::Multiple => false,
        }
    }
}

/// Looks for forks, created by `eq_idx` validator, that are visible in the past of the panorama.
///
/// Exits early if more than one fork is found as it cannot satisfy LNC then anyway.
pub(crate) fn find_forks<C: Context>(
    panorama: &Panorama<C>,
    endorsed: &BTreeSet<C::Hash>,
    eq_idx: ValidatorIndex,
    state: &State<C>,
) -> LncForks<C> {
    // Find all forks by eq_idx that are cited naively by wunit itself.
    // * If it's more than one then LNC is violated.
    // * If it's none, LNC is not violated: If the LNC were violated, it would be because of two
    //   naive citations by wunit.creator's earlier units. So the latest of those earlier units
    //   would already be violating the LNC itself, and thus would not have been added to the state.
    // * Otherwise store the unique naively cited fork in naive_by_wunit.
    let mut opt_naive_by_wunit = None;

    // Returns true if any endorsed (according to wunit) unit cites the given unit.
    let seen_by_endorsed = |hash| endorsed.iter().any(|e_hash| state.sees(e_hash, hash));

    // Iterate over all units cited by wunit.
    let mut to_visit: Vec<&C::Hash> = panorama.iter_correct_hashes().collect();
    // This set is a filter so that units don't get added to to_visit twice.
    let mut added_to_to_visit: HashSet<_> = to_visit.iter().cloned().collect();
    while let Some(hash) = to_visit.pop() {
        if seen_by_endorsed(hash) {
            continue; // This unit and everything below is not cited naively.
        }
        let unit = state.unit(hash);
        match &unit.panorama[eq_idx] {
            Observation::Correct(eq_hash) => {
                // The unit (and everything it cites) can only see a single fork.
                // No need to traverse further downward.
                if !seen_by_endorsed(eq_hash) {
                    // The fork is cited naively!
                    match opt_naive_by_wunit {
                        // It's the first naively cited fork we found.
                        None => opt_naive_by_wunit = Some(eq_hash),
                        Some(other_hash) => {
                            // If eq_hash is later than other_hash, it is the tip of the
                            // same fork. If it is earlier, then other_hash is the tip.
                            if state.sees_correct(eq_hash, other_hash) {
                                opt_naive_by_wunit = Some(eq_hash);
                            } else if !state.sees_correct(other_hash, eq_hash) {
                                return LncForks::Multiple; // We found two incompatible forks!
                            }
                        }
                    }
                }
            }
            // No forks are cited by this unit. No need to traverse further.
            Observation::None => (),
            // The unit still sees the equivocator as faulty: We need to traverse further
            // down the graph to find all cited forks.
            Observation::Faulty => to_visit.extend(
                unit.panorama
                    .iter_correct_hashes()
                    .filter(|hash| added_to_to_visit.insert(hash))
                    .cloned()
                    .into_iter(),
            ),
        }
    }

    match opt_naive_by_wunit {
        None => LncForks::None,
        Some(uhash) => LncForks::Single(*uhash),
    }
}
