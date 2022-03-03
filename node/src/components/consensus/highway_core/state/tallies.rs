use std::{
    collections::BTreeMap,
    iter::{self, Extend, FromIterator},
    ops::Index,
};

use crate::components::consensus::{
    highway_core::state::{State, Weight},
    traits::Context,
};

/// A tally of votes at a specific height. This is never empty: It contains at least one vote.
///
/// It must always contain at most one vote from each validator. In particular, the sum of the
/// weights must be at most the total of all validators' weights.
#[derive(Clone)]
pub(crate) struct Tally<'a, C: Context> {
    /// The block with the highest weight, and the highest hash if there's a tie.
    max: (Weight, &'a C::Hash),
    /// The total vote weight for each block.
    votes: BTreeMap<&'a C::Hash, Weight>,
}

impl<'a, C: Context> Extend<(&'a C::Hash, Weight)> for Tally<'a, C> {
    fn extend<T: IntoIterator<Item = (&'a C::Hash, Weight)>>(&mut self, iter: T) {
        for (bhash, w) in iter {
            self.add(bhash, w);
        }
    }
}

impl<'a, 'b, C: Context> IntoIterator for &'b Tally<'a, C> {
    type Item = (&'a C::Hash, Weight);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'b>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.votes.iter().map(|(b, w)| (*b, *w)))
    }
}

impl<'a, C: Context> Tally<'a, C> {
    /// Returns a new tally with a single entry.
    fn new(bhash: &'a C::Hash, w: Weight) -> Self {
        Tally {
            max: (w, bhash),
            votes: iter::once((bhash, w)).collect(),
        }
    }

    /// Creates a tally from a list of units. Returns `None` if the iterator is empty.
    fn try_from_iter<T: IntoIterator<Item = (&'a C::Hash, Weight)>>(iter: T) -> Option<Self> {
        let mut iter = iter.into_iter();
        let (bhash, w) = iter.next()?;
        let mut tally = Tally::new(bhash, w);
        tally.extend(iter);
        Some(tally)
    }

    /// Returns a new tally with the same votes, but one level lower: a vote for a block counts as
    /// a vote for that block's parent. Panics if called on level 0.
    ///
    /// This preserves the total weight, and the set of validators who contribute to that weight.
    fn parents(&self, state: &'a State<C>) -> Self {
        let to_parent = |(h, w): (&&'a C::Hash, &Weight)| (state.block(*h).parent().unwrap(), *w);
        // safe as Tally is never empty.
        Self::try_from_iter(self.votes.iter().map(to_parent)).unwrap()
    }

    /// Adds a vote for a block to the tally, possibly updating the current maximum.
    fn add(&mut self, bhash: &'a C::Hash, weight: Weight) {
        let w = self.votes.entry(bhash).or_default();
        *w += weight;
        self.max = (*w, bhash).max(self.max);
    }

    /// Returns the total weight of the votes included in this tally.
    fn weight(&self) -> Weight {
        self.votes.values().cloned().sum()
    }

    /// Returns the maximum voting weight a single block received.
    fn max_w(&self) -> Weight {
        self.max.0
    }

    /// Returns the block hash that received the most votes; the highest hash in case of a tie.
    fn max_bhash(&self) -> &'a C::Hash {
        self.max.1
    }

    /// Returns a tally containing only the votes for descendants of `bhash`.
    ///
    /// The total weight of the result is less than or equal to the total weight of `self`, and the
    /// set of validators contributing to it is a subset of the ones contributing to `self`.
    fn filter_descendants(
        self,
        height: u64,
        bhash: &'a C::Hash,
        state: &'a State<C>,
    ) -> Option<Self> {
        let iter = self.votes.into_iter();
        Self::try_from_iter(
            iter.filter(|&(b, _)| state.find_ancestor_proposal(b, height) == Some(bhash)),
        )
    }
}

/// A list of tallies by block height. The tally at each height contains only the units that point
/// directly to a block at that height, not at a descendant.
///
/// Each validator must contribute their weight to at most one entry: The height of the block that
/// they most recently voted for.
pub(crate) struct Tallies<'a, C: Context>(BTreeMap<u64, Tally<'a, C>>);

impl<'a, C: Context> Default for Tallies<'a, C> {
    fn default() -> Self {
        Tallies(BTreeMap::new())
    }
}

impl<'a, C: Context> Index<u64> for Tallies<'a, C> {
    type Output = Tally<'a, C>;

    fn index(&self, index: u64) -> &Self::Output {
        &self.0[&index]
    }
}

impl<'a, C: Context> FromIterator<(u64, &'a C::Hash, Weight)> for Tallies<'a, C> {
    fn from_iter<T: IntoIterator<Item = (u64, &'a C::Hash, Weight)>>(iter: T) -> Self {
        let mut tallies = Self::default();
        for (height, bhash, weight) in iter {
            tallies.add(height, bhash, weight);
        }
        tallies
    }
}

impl<'a, C: Context> Tallies<'a, C> {
    /// Returns the height and hash of a block that is an ancestor of the fork choice, and _not_ an
    /// ancestor of all entries in `self`. Returns `None` if `self` is empty.
    pub(crate) fn find_decided(&self, state: &'a State<C>) -> Option<(u64, &'a C::Hash)> {
        let max_height = *self.0.keys().next_back()?;
        let total_weight: Weight = self.0.values().map(Tally::weight).sum();
        // In the loop, this will be the tally of all votes from higher than the current height.
        let mut prev_tally = self[max_height].clone();
        // Start from `max_height - 1` and find the greatest height where a decision can be made.
        for height in (0..max_height).rev() {
            // The tally at `height` is the sum of the parents of `height + 1` and the units that
            // point directly to blocks at `height`.
            let mut h_tally = prev_tally.parents(state);
            if let Some(tally) = self.0.get(&height) {
                h_tally.extend(tally);
            }
            // If any block received more than 50%, a decision can be made: Either that block is
            // the fork choice, or we can pick its highest scoring child from `prev_tally`.
            if h_tally.max_w() > total_weight / 2 {
                #[allow(clippy::integer_arithmetic)] // height < max_height, so height < u64::MAX
                return Some(
                    match prev_tally.filter_descendants(height, h_tally.max_bhash(), state) {
                        Some(filtered) => (height + 1, filtered.max_bhash()),
                        None => (height, h_tally.max_bhash()),
                    },
                );
            }
            prev_tally = h_tally;
        }
        // Even at level 0 no block received a majority. Pick the one with the highest weight.
        Some((0, prev_tally.max_bhash()))
    }

    /// Removes all votes for blocks that are not descendants of `bhash`.
    pub(crate) fn filter_descendants(
        self,
        height: u64,
        bhash: &'a C::Hash,
        state: &'a State<C>,
    ) -> Self {
        // Each tally will be filtered to remove blocks incompatible with `bhash`.
        let map_compatible = |(h, t): (u64, Tally<'a, C>)| {
            t.filter_descendants(height, bhash, state).map(|t| (h, t))
        };
        // All tallies at `height` and lower can be removed, too.
        let relevant_heights = self.0.into_iter().rev().take_while(|(h, _)| *h > height);
        Tallies(relevant_heights.filter_map(map_compatible).collect())
    }

    /// Adds an entry to the tally at the specified `height`.
    fn add(&mut self, height: u64, bhash: &'a C::Hash, weight: Weight) {
        self.0
            .entry(height)
            .and_modify(|tally| tally.add(bhash, weight))
            .or_insert_with(|| Tally::new(bhash, weight));
    }

    /// Returns `true` if there are no tallies in this map.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{tests::*, State},
        *,
    };

    impl<'a> Tallies<'a, TestContext> {
        /// Returns the number of tallies.
        pub(crate) fn len(&self) -> usize {
            self.0.len()
        }
    }

    #[test]
    fn tallies() -> Result<(), AddUnitError<TestContext>> {
        let mut state = State::new_test(WEIGHTS, 0);

        // Create blocks with scores as follows:
        //
        //          a0: 7 — a1: 3
        //        /       \
        // b0: 12           b2: 4
        //        \
        //          c0: 5 — c1: 5
        let b0 = add_unit!(state, BOB, 0xB0; N, N, N)?;
        let c0 = add_unit!(state, CAROL, 0xC0; N, b0, N)?;
        let c1 = add_unit!(state, CAROL, 0xC1; N, b0, c0)?;
        let a0 = add_unit!(state, ALICE, 0xA0; N, b0, N)?;
        let b1 = add_unit!(state, BOB, None; a0, b0, N)?; // Just a ballot; not shown above.
        let a1 = add_unit!(state, ALICE, 0xA1; a0, b1, c1)?;
        let b2 = add_unit!(state, BOB, 0xB2; a0, b1, N)?;

        // These are the entries of a panorama seeing `a1`, `b2` and `c0`.
        let vote_entries = vec![
            (1, &c0, Weight(5)),
            (2, &a1, Weight(3)),
            (2, &b2, Weight(4)),
        ];
        let tallies: Tallies<TestContext> = vote_entries.into_iter().collect();
        assert_eq!(2, tallies.len());
        assert_eq!(Weight(5), tallies[1].weight()); // Carol's unit is on height 1.
        assert_eq!(Weight(7), tallies[2].weight()); // Alice's and Bob's units are on height 2.

        // Compute the tally at height 1: Take the parents of the blocks Alice and Bob vote for...
        let mut h1_tally = tallies[2].parents(&state);
        // (Their units have the same parent: `a0`.)
        assert_eq!(1, h1_tally.votes.len());
        assert_eq!(Weight(7), h1_tally.votes[&a0]);
        // ...and adding Carol's vote.
        h1_tally.extend(&tallies[1]);
        assert_eq!(2, h1_tally.votes.len());
        assert_eq!(Weight(5), h1_tally.votes[&c0]);

        // `find_decided` finds the fork choice in one step: On height 1, `a0` has the majority. On
        // height 2, the child of `a0` with the highest score is `b2`.
        assert_eq!(Some((2, &b2)), tallies.find_decided(&state));

        // But let's filter at level 1, and keep only the children of `a0`:
        let tallies = tallies.filter_descendants(1, &a0, &state);
        assert_eq!(1, tallies.len());
        assert_eq!(2, tallies[2].votes.len());
        assert_eq!(Weight(3), tallies[2].votes[&a1]);
        assert_eq!(Weight(4), tallies[2].votes[&b2]);
        Ok(())
    }

    #[test]
    fn tally_try_from_iter() {
        let tally: Option<Tally<TestContext>> = Tally::try_from_iter(vec![]);
        assert!(tally.is_none());
        let votes = vec![
            (&10, Weight(2)),
            (&20, Weight(3)),
            (&10, Weight(4)),
            (&30, Weight(5)),
            (&20, Weight(6)),
        ];
        let tally: Tally<TestContext> = Tally::try_from_iter(votes).unwrap();
        assert_eq!(Weight(9), tally.max_w());
        assert_eq!(&20, tally.max_bhash());
        assert_eq!(Weight(20), tally.weight());
        assert_eq!(Weight(6), tally.votes[&10]);
    }
}
