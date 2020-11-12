use std::collections::HashMap;

use crate::components::consensus::{highway_core::validators::ValidatorIndex, traits::Context};

use super::{Panorama, State};

/// Helper structure used for computing which votes need endorsement.
pub(crate) struct EndorsementTracker<'a, C: Context> {
    /// Validators that are known to be equivocators by the initial panorama.
    known_equivocators: Vec<ValidatorIndex>,
    /// Visited units. Used for avoding visitng the same unit twice.
    visited_nodes: Vec<C::Hash>,
    /// Collection from a unit hash to indexes of equivocators that this unit cites as honest.
    equiv_citations: HashMap<C::Hash, Vec<ValidatorIndex>>,
    /// Read-only state.
    state: &'a State<C>,
}

impl<'a, C: Context> EndorsementTracker<'a, C> {
    pub(crate) fn new(state: &'a State<C>) -> Self {
        EndorsementTracker {
            known_equivocators: Vec::new(),
            visited_nodes: Vec::new(),
            equiv_citations: HashMap::new(),
            state,
        }
    }

    /// Returns votes that cite equivocating validators as honest.
    pub(crate) fn run(mut self, panorama: &Panorama<C>) -> HashMap<C::Hash, Vec<ValidatorIndex>> {
        self.known_equivocators = panorama.iter_faulty_validators().collect();
        if self.known_equivocators.is_empty() {
            return HashMap::new();
        }
        for v in panorama.iter_correct_hashes() {
            self.find_equivocations(&v)
        }
        self.equiv_citations
    }

    /// Looks for equivocations in a downward set of vote.
    fn find_equivocations(&mut self, vhash: &C::Hash) {
        self.visited_nodes.push(*vhash);
        let cited = self.cited_equivocators(vhash);
        if !cited.is_empty() {
            self.equiv_citations.insert(*vhash, cited);
        }

        // Units seen as correcy by panorama of `vhash`.
        let to_visit: Vec<C::Hash> = self
            .state
            .vote(vhash)
            .panorama
            .iter_correct_hashes()
            .filter(|&v| {
                // Do not follow units that are created by known equivocators,
                // have already been seen as endorsed by this node's state
                // or has already been visited.
                !self.by_equivocator(v) || !self.state.is_endorsed(v) || !self.already_visited(v)
            })
            .cloned()
            .collect();

        for v in to_visit {
            self.find_equivocations(&v);
        }
    }

    /// Returns which of the known equivocators this vote cites as correct.
    fn cited_equivocators(&self, vhash: &C::Hash) -> Vec<ValidatorIndex> {
        self.state
            .vote(vhash)
            .panorama
            .iter_correct_hashes()
            .filter_map(|hash| {
                let creator = self.state.vote(hash).creator;
                if self.known_equivocator(creator) {
                    Some(creator)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns whether validator is known equivocator according to `self`.
    fn known_equivocator(&self, vidx: ValidatorIndex) -> bool {
        self.known_equivocators
            .iter()
            .find(|&idx| *idx == vidx)
            .is_some()
    }

    /// Return whether unit was created by known equivocator.
    fn by_equivocator(&self, vhash: &C::Hash) -> bool {
        self.known_equivocator(self.state.vote(vhash).creator)
    }

    /// Returns whether we had already visited that unit.
    fn already_visited(&self, vhash: &C::Hash) -> bool {
        self.visited_nodes.iter().find(|&v| v == vhash).is_some()
    }
}
