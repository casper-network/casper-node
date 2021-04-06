use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
};

use datasize::DataSize;
use itertools::Itertools;
use once_cell::sync::Lazy;
use tracing::{debug, warn};

use casper_types::{PublicKey, U512};

use crate::{
    components::consensus::{
        cl_context::ClContext,
        consensus_protocol::{ConsensusProtocol, ProposedBlock},
        protocols::highway::HighwayProtocol,
    },
    types::Timestamp,
};

const CASPER_ENABLE_DETAILED_CONSENSUS_METRICS_ENV_VAR: &str =
    "CASPER_ENABLE_DETAILED_CONSENSUS_METRICS";
static CASPER_ENABLE_DETAILED_CONSENSUS_METRICS: Lazy<bool> =
    Lazy::new(|| env::var(CASPER_ENABLE_DETAILED_CONSENSUS_METRICS_ENV_VAR).is_ok());

/// A proposed block waiting for validation and dependencies.
#[derive(DataSize)]
pub struct PendingCandidate {
    /// Whether the proto block has been validated yet.
    validated: bool,
    /// A list of IDs of accused validators for which we are still missing evidence.
    missing_evidence: Vec<PublicKey>,
}

impl PendingCandidate {
    fn new(missing_evidence: Vec<PublicKey>) -> Self {
        PendingCandidate {
            validated: false,
            missing_evidence,
        }
    }

    fn is_complete(&self) -> bool {
        self.validated && self.missing_evidence.is_empty()
    }
}

pub struct Era<I> {
    /// The consensus protocol instance.
    pub(crate) consensus: Box<dyn ConsensusProtocol<I, ClContext>>,
    /// The scheduled starting time of this era.
    pub(crate) start_time: Timestamp,
    /// The height of this era's first block.
    pub(crate) start_height: u64,
    /// Pending candidate blocks, waiting for validation. The boolean is `true` if the proto block
    /// has been validated; the map contains the list of accused validators missing evidence.
    candidates: HashMap<ProposedBlock<ClContext>, PendingCandidate>,
    /// Validators banned in this and the next BONDED_ERAS eras, because they were slashed in the
    /// previous switch block.
    pub(crate) newly_slashed: Vec<PublicKey>,
    /// Validators that have been slashed in any of the recent BONDED_ERAS switch blocks. This
    /// includes `newly_slashed`.
    pub(crate) slashed: HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    accusations: HashSet<PublicKey>,
    /// The validator weights.
    validators: BTreeMap<PublicKey, U512>,
}

impl<I> Era<I> {
    pub(crate) fn new(
        consensus: Box<dyn ConsensusProtocol<I, ClContext>>,
        start_time: Timestamp,
        start_height: u64,
        newly_slashed: Vec<PublicKey>,
        slashed: HashSet<PublicKey>,
        validators: BTreeMap<PublicKey, U512>,
    ) -> Self {
        Era {
            consensus,
            start_time,
            start_height,
            candidates: HashMap::new(),
            newly_slashed,
            slashed,
            accusations: HashSet::new(),
            validators,
        }
    }

    /// Adds a new candidate block, together with the accusations for which we don't have evidence
    /// yet.
    pub(crate) fn add_candidate(
        &mut self,
        proposed_block: ProposedBlock<ClContext>,
        missing_evidence: Vec<PublicKey>,
    ) {
        self.candidates
            .insert(proposed_block, PendingCandidate::new(missing_evidence));
    }

    /// Marks the dependencies of candidate blocks on evidence against validator `pub_key` as
    /// resolved and returns all candidates that have no missing dependencies left.
    pub(crate) fn resolve_evidence(
        &mut self,
        pub_key: &PublicKey,
    ) -> Vec<ProposedBlock<ClContext>> {
        for pc in self.candidates.values_mut() {
            pc.missing_evidence.retain(|pk| pk != pub_key);
        }
        self.consensus.mark_faulty(pub_key);
        let (complete, candidates): (HashMap<_, _>, HashMap<_, _>) = self
            .candidates
            .drain()
            .partition(|(_, candidate)| candidate.is_complete());
        self.candidates = candidates;
        complete
            .into_iter()
            .map(|(proposed_block, _)| proposed_block)
            .collect()
    }

    /// Marks the proto block as valid or invalid, and returns all candidates whose validity is now
    /// fully determined.
    pub(crate) fn resolve_validity(
        &mut self,
        proposed_block: &ProposedBlock<ClContext>,
        valid: bool,
    ) -> bool {
        if valid {
            self.accept_candidate_block(proposed_block)
        } else {
            self.reject_candidate_block(proposed_block)
        }
    }

    /// Marks the dependencies of candidate blocks on the validity of the specified proto block as
    /// resolved and returns `true` if the block was present and has no missing dependencies.
    pub(crate) fn accept_candidate_block(
        &mut self,
        proposed_block: &ProposedBlock<ClContext>,
    ) -> bool {
        if let Some(pc) = self.candidates.get_mut(proposed_block) {
            if !pc.missing_evidence.is_empty() {
                pc.validated = true;
                return false;
            }
        }
        self.candidates.remove(proposed_block).is_some()
    }

    /// Removes the invalid proposed block. Returns `true` if it was present.
    pub(crate) fn reject_candidate_block(
        &mut self,
        proposed_block: &ProposedBlock<ClContext>,
    ) -> bool {
        self.candidates.remove(proposed_block).is_some()
    }

    /// Adds new accusations from a finalized block.
    pub(crate) fn add_accusations(&mut self, accusations: &[PublicKey]) {
        for pub_key in accusations {
            if !self.slashed.contains(pub_key) {
                self.accusations.insert(pub_key.clone());
            }
        }
    }

    /// Returns all accusations from finalized blocks so far.
    pub(crate) fn accusations(&self) -> Vec<PublicKey> {
        self.accusations.iter().cloned().sorted().collect()
    }

    /// Returns the map of validator weights.
    pub(crate) fn validators(&self) -> &BTreeMap<PublicKey, U512> {
        &self.validators
    }

    /// Sets the pause status: While paused we don't create consensus messages other than pings.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.consensus.set_paused(paused);
    }
}

impl<I> DataSize for Era<I>
where
    I: DataSize + 'static,
{
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    #[inline]
    fn estimate_heap_size(&self) -> usize {
        // Destructure self, so we can't miss any fields.
        let Era {
            consensus,
            start_time,
            start_height,
            candidates,
            newly_slashed,
            slashed,
            accusations,
            validators,
        } = self;

        // `DataSize` cannot be made object safe due its use of associated constants. We implement
        // it manually here, downcasting the consensus protocol as a workaround.

        let consensus_heap_size = {
            let any_ref = consensus.as_any();

            if let Some(highway) = any_ref.downcast_ref::<HighwayProtocol<I, ClContext>>() {
                if *CASPER_ENABLE_DETAILED_CONSENSUS_METRICS {
                    let detailed = (*highway).estimate_detailed_heap_size();
                    match serde_json::to_string(&detailed) {
                        Ok(encoded) => debug!(%encoded, "consensus memory metrics"),
                        Err(err) => warn!(%err, "error encoding consensus memory metrics"),
                    }
                    detailed.total()
                } else {
                    (*highway).estimate_heap_size()
                }
            } else {
                warn!(
                    "could not downcast consensus protocol to \
                    HighwayProtocol<I, ClContext> to determine heap allocation size"
                );
                0
            }
        };

        consensus_heap_size
            .saturating_add(start_time.estimate_heap_size())
            .saturating_add(start_height.estimate_heap_size())
            .saturating_add(candidates.estimate_heap_size())
            .saturating_add(newly_slashed.estimate_heap_size())
            .saturating_add(slashed.estimate_heap_size())
            .saturating_add(accusations.estimate_heap_size())
            .saturating_add(validators.estimate_heap_size())
    }
}
